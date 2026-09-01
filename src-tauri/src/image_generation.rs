use crate::app_database::load_feature_flags;
use crate::database::{new_id, now, open_database, AppResult};
use crate::mutation::{execute_mutations_in_transaction, MutationRequest};
use crate::provider::{load_provider, provider_secret, ProviderConfig};
use base64::Engine;
use reqwest::{multipart, Client};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

#[derive(Clone, Default)]
pub struct ImageGenerationState {
    cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOptions {
    pub size: Option<String>,
    pub quality: Option<String>,
    pub count: Option<usize>,
    pub background: Option<String>,
    pub mock_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageInput {
    pub request_id: String,
    pub target_type: String,
    pub target_id: String,
    pub provider_id: String,
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub reference_images: Vec<String>,
    pub options: ImageOptions,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResult {
    pub id: String,
    pub job_id: String,
    pub file_path: String,
    pub preview_path: Option<String>,
    pub metadata: Value,
    pub sort_order: i64,
    pub selection_state: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageJob {
    pub id: String,
    pub target_type: String,
    pub target_id: String,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub prompt_revision: i64,
    pub reference_images: Vec<String>,
    pub options: ImageOptions,
    pub status: String,
    pub usage: Value,
    pub error: Option<Value>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub results: Vec<ImageResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectImageResult {
    pub formal_path: String,
    pub formal_object_id: String,
    pub revision: i64,
}

#[derive(Debug)]
struct GeneratedImage {
    bytes: Vec<u8>,
    extension: &'static str,
    metadata: Value,
}

#[derive(Debug)]
struct ProviderBatch {
    images: Vec<GeneratedImage>,
    usage: Value,
    error: Option<String>,
}

#[derive(Debug)]
struct ReferenceImage {
    bytes: Vec<u8>,
    file_name: String,
    mime_type: &'static str,
}

trait ImageGenerationProvider {
    fn generate(
        &self,
        prompt: &str,
        model: &str,
        options: &ImageOptions,
        references: &[ReferenceImage],
        cancelled: &AtomicBool,
    ) -> AppResult<ProviderBatch>;
}

struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    secret: String,
}

impl ImageGenerationProvider for OpenAiCompatibleProvider {
    fn generate(
        &self,
        prompt: &str,
        model: &str,
        options: &ImageOptions,
        references: &[ReferenceImage],
        cancelled: &AtomicBool,
    ) -> AppResult<ProviderBatch> {
        if cancelled.load(Ordering::SeqCst) {
            return Err("TASK_CANCELLED: 用户已取消生图任务".into());
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_seconds as u64))
            .build()
            .map_err(|e| format!("创建 Provider 客户端失败：{e}"))?;
        let base_url = self.config.base_url.trim_end_matches('/');
        let request = if references.is_empty() {
            client
                .post(format!("{base_url}/images/generations"))
                .bearer_auth(&self.secret)
                .json(&json!({
                    "model": model,
                    "prompt": prompt,
                    "n": options.count.unwrap_or(1),
                    "size": options.size.as_deref().unwrap_or("1024x1024"),
                    "quality": options.quality.as_deref().unwrap_or("auto"),
                    "background": options.background.as_deref().unwrap_or("auto"),
                    "response_format": "b64_json",
                    "output_format": "png"
                }))
        } else {
            let mut form = multipart::Form::new()
                .text("model", model.to_string())
                .text("prompt", prompt.to_string())
                .text("n", options.count.unwrap_or(1).to_string())
                .text(
                    "size",
                    options.size.as_deref().unwrap_or("1024x1024").to_string(),
                )
                .text(
                    "quality",
                    options.quality.as_deref().unwrap_or("auto").to_string(),
                )
                .text("response_format", "b64_json");
            for reference in references {
                let part = multipart::Part::bytes(reference.bytes.clone())
                    .file_name(reference.file_name.clone())
                    .mime_str(reference.mime_type)
                    .map_err(|e| format!("参考图 MIME 类型无效：{e}"))?;
                form = form.part("image", part);
            }
            client
                .post(format!("{base_url}/images/edits"))
                .bearer_auth(&self.secret)
                .multipart(form)
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("创建 Provider 异步运行时失败：{e}"))?;
        let (status, value) = runtime.block_on(async {
            let response = tokio::select! {
                response = request.send() => response.map_err(|e| {
                    if e.is_timeout() {
                        "PROVIDER_TIMEOUT: 生图请求超时".into()
                    } else {
                        format!("PROVIDER_NETWORK_FAILED: {e}")
                    }
                })?,
                _ = wait_for_cancellation(cancelled) => {
                    return Err("TASK_CANCELLED: 用户已取消生图任务".into());
                }
            };
            let status = response.status();
            let value: Value = response
                .json()
                .await
                .map_err(|e| format!("Provider 返回了无效 JSON：{e}"))?;
            Ok::<_, String>((status, value))
        })?;
        if !status.is_success() {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Provider 请求失败");
            return Err(match status.as_u16() {
                401 | 403 => "PROVIDER_AUTH_FAILED: Provider 拒绝了当前密钥".into(),
                429 => "PROVIDER_RATE_LIMITED: Provider 当前限流，请稍后重试".into(),
                400 => format!("PROVIDER_REJECTED: {message}"),
                code => format!("PROVIDER_FAILED({code}): {message}"),
            });
        }
        let mut images = Vec::new();
        let mut invalid = 0;
        for (index, item) in value
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(encoded) = item.get("b64_json").and_then(Value::as_str) else {
                invalid += 1;
                continue;
            };
            match base64::engine::general_purpose::STANDARD.decode(encoded) {
                Ok(bytes) if valid_image_bytes(&bytes) => images.push(GeneratedImage { bytes, extension: "png", metadata: json!({"providerIndex": index, "revisedPrompt": item.get("revised_prompt")}) }),
                _ => invalid += 1,
            }
        }
        if images.is_empty() {
            return Err("PROVIDER_RESULT_INVALID: Provider 未返回有效图片".into());
        }
        Ok(ProviderBatch {
            images,
            usage: value.get("usage").cloned().unwrap_or_else(|| json!({})),
            error: (invalid > 0).then(|| format!("{invalid} 个结果无效")),
        })
    }
}

async fn wait_for_cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct MockProvider;

impl ImageGenerationProvider for MockProvider {
    fn generate(
        &self,
        _prompt: &str,
        _model: &str,
        options: &ImageOptions,
        _references: &[ReferenceImage],
        cancelled: &AtomicBool,
    ) -> AppResult<ProviderBatch> {
        if cancelled.load(Ordering::SeqCst) || options.mock_mode.as_deref() == Some("cancel") {
            return Err("TASK_CANCELLED: 用户已取消生图任务".into());
        }
        if options.mock_mode.as_deref() == Some("fail") {
            return Err("PROVIDER_REJECTED: Mock Provider 拒绝请求".into());
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .map_err(|e| e.to_string())?;
        let count = options.count.unwrap_or(1).clamp(1, 4);
        Ok(ProviderBatch {
            images: (0..count)
                .map(|index| GeneratedImage {
                    bytes: png.clone(),
                    extension: "png",
                    metadata: json!({"mock": true, "providerIndex": index}),
                })
                .collect(),
            usage: json!({"costLevel": "mock"}),
            error: (options.mock_mode.as_deref() == Some("partial"))
                .then(|| "Mock 部分结果成功".into()),
        })
    }
}

fn valid_image_bytes(bytes: &[u8]) -> bool {
    bytes.len() <= 30 * 1024 * 1024
        && (bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || bytes.starts_with(b"\xff\xd8\xff")
            || (bytes.len() > 12 && &bytes[8..12] == b"WEBP"))
}

fn image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn validate_options(options: &ImageOptions) -> AppResult<()> {
    if !matches!(options.count.unwrap_or(1), 1..=4) {
        return Err("TOOL_ARGUMENT_INVALID: 单次候选数量必须为 1–4".into());
    }
    if let Some(size) = options.size.as_deref() {
        if !["auto", "1024x1024", "1024x1536", "1536x1024"].contains(&size) {
            return Err("TOOL_ARGUMENT_INVALID: 不支持的图片尺寸".into());
        }
    }
    Ok(())
}

fn prompt_revision(conn: &Connection, target_type: &str, target_id: &str) -> AppResult<i64> {
    let exists: bool = match target_type {
        "assetRequirement" => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM asset_requirements WHERE id=?1)",
            [target_id],
            |row| row.get(0),
        ),
        "keyframe" => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM keyframes WHERE id=?1)",
            [target_id],
            |row| row.get(0),
        ),
        _ => {
            return Err("TOOL_ARGUMENT_INVALID: 生图目标必须是 assetRequirement 或 keyframe".into())
        }
    }
    .map_err(|e| e.to_string())?;
    if !exists {
        return Err("OBJECT_NOT_FOUND: 生图目标不存在".into());
    }
    conn.query_row("SELECT revision FROM projects LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|e| e.to_string())
}

fn validate_references(conn: &Connection, project: &Path, references: &[String]) -> AppResult<()> {
    if references.len() > 8 {
        return Err("TOOL_ARGUMENT_INVALID: 参考图最多 8 张".into());
    }
    for relative in references {
        if Path::new(relative).is_absolute() || relative.contains("..") {
            return Err("TOOL_ARGUMENT_INVALID: 参考图必须是项目内相对路径".into());
        }
        let formal: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM asset_media WHERE file_path=?1 UNION SELECT 1 FROM keyframes WHERE file_path=?1)",
            [relative], |row| row.get(0)).map_err(|e| e.to_string())?;
        let explicit_reference = relative.replace('\\', "/").starts_with("references/");
        if (!formal && !explicit_reference) || !project.join(relative).is_file() {
            return Err(
                "TOOL_ARGUMENT_INVALID: 参考图必须来自正式资产、当前关键帧或明确选择的参考资料"
                    .into(),
            );
        }
    }
    Ok(())
}

fn load_reference_images(project: &Path, references: &[String]) -> AppResult<Vec<ReferenceImage>> {
    let project_root = project
        .canonicalize()
        .map_err(|e| format!("读取项目目录失败：{e}"))?;
    references
        .iter()
        .map(|relative| {
            let absolute = project
                .join(relative)
                .canonicalize()
                .map_err(|e| format!("读取参考图失败（{relative}）：{e}"))?;
            if !absolute.starts_with(&project_root) {
                return Err("TOOL_ARGUMENT_INVALID: 参考图必须位于当前项目内".into());
            }
            let bytes =
                fs::read(&absolute).map_err(|e| format!("读取参考图失败（{relative}）：{e}"))?;
            if !valid_image_bytes(&bytes) {
                return Err(format!(
                    "TOOL_ARGUMENT_INVALID: 参考图不是有效的 PNG、JPEG 或 WebP（{relative}）"
                ));
            }
            let mime_type = image_mime_type(&bytes).expect("validated image type");
            let file_name = absolute
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("reference.png")
                .to_string();
            Ok(ReferenceImage {
                bytes,
                file_name,
                mime_type,
            })
        })
        .collect()
}

fn load_job(conn: &Connection, job_id: &str) -> AppResult<ImageJob> {
    let row = conn.query_row(
        "SELECT id,target_type,target_id,provider,model,prompt,prompt_revision,reference_images_json,options_json,status,usage_json,error_json,created_at,started_at,completed_at FROM image_generation_jobs WHERE id=?1",
        [job_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,i64>(6)?,row.get::<_,String>(7)?,row.get::<_,String>(8)?,row.get::<_,String>(9)?,row.get::<_,String>(10)?,row.get::<_,Option<String>>(11)?,row.get::<_,String>(12)?,row.get::<_,Option<String>>(13)?,row.get::<_,Option<String>>(14)?)))
        .map_err(|_| "OBJECT_NOT_FOUND: 生图任务不存在".to_string())?;
    let mut stmt = conn.prepare("SELECT id,job_id,file_path,preview_path,metadata_json,sort_order,selection_state,created_at FROM image_generation_results WHERE job_id=?1 ORDER BY sort_order,id").map_err(|e| e.to_string())?;
    let results = stmt
        .query_map([job_id], |r| {
            Ok(ImageResult {
                id: r.get(0)?,
                job_id: r.get(1)?,
                file_path: r.get(2)?,
                preview_path: r.get(3)?,
                metadata: serde_json::from_str(&r.get::<_, String>(4)?)
                    .unwrap_or_else(|_| json!({})),
                sort_order: r.get(5)?,
                selection_state: r.get(6)?,
                created_at: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ImageJob {
        id: row.0,
        target_type: row.1,
        target_id: row.2,
        provider: row.3,
        model: row.4,
        prompt: row.5,
        prompt_revision: row.6,
        reference_images: serde_json::from_str(&row.7).unwrap_or_default(),
        options: serde_json::from_str(&row.8).map_err(|e| e.to_string())?,
        status: row.9,
        usage: serde_json::from_str(&row.10).unwrap_or_else(|_| json!({})),
        error: row.11.and_then(|v| serde_json::from_str(&v).ok()),
        created_at: row.12,
        started_at: row.13,
        completed_at: row.14,
        results,
    })
}

fn create_job(project: &Path, input: &GenerateImageInput) -> AppResult<ImageJob> {
    validate_options(&input.options)?;
    if input.request_id.trim().is_empty() || input.prompt.trim().is_empty() {
        return Err("TOOL_ARGUMENT_INVALID: requestId 和提示词不能为空".into());
    }
    let conn = open_database(project)?;
    if let Ok(existing) = load_job(&conn, &input.request_id) {
        return Ok(existing);
    }
    validate_references(&conn, project, &input.reference_images)?;
    let revision = prompt_revision(&conn, &input.target_type, &input.target_id)?;
    let model = input.model.clone().unwrap_or_default();
    let timestamp = now();
    conn.execute(
        "INSERT INTO image_generation_jobs (id,target_type,target_id,provider,model,prompt,prompt_revision,reference_images_json,options_json,status,request_json,usage_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'queued',?10,'{}',?11)",
        params![input.request_id,input.target_type,input.target_id,input.provider_id,model,input.prompt.trim(),revision,serde_json::to_string(&input.reference_images).map_err(|e|e.to_string())?,serde_json::to_string(&input.options).map_err(|e|e.to_string())?,json!({"targetType":input.target_type,"targetId":input.target_id,"referenceCount":input.reference_images.len()}).to_string(),timestamp]
    ).map_err(|e|format!("创建生图任务失败：{e}"))?;
    load_job(&conn, &input.request_id)
}

fn run_job(
    app_data_dir: &Path,
    project: &Path,
    job_id: &str,
    provider: &dyn ImageGenerationProvider,
    cancelled: &AtomicBool,
) -> AppResult<ImageJob> {
    let mut conn = open_database(project)?;
    let job = load_job(&conn, job_id)?;
    if ["completed", "partial", "cancelled", "failed"].contains(&job.status.as_str()) {
        return Ok(job);
    }
    conn.execute(
        "UPDATE image_generation_jobs SET status='running', started_at=COALESCE(started_at,?1) WHERE id=?2 AND status IN ('created','queued','interrupted')",
        params![now(), job_id],
    )
    .map_err(|e| e.to_string())?;
    let model = if job.model.is_empty() {
        load_provider(app_data_dir, &job.provider)?.default_model
    } else {
        job.model.clone()
    };
    let references = load_reference_images(project, &job.reference_images)?;
    let batch = match provider.generate(&job.prompt, &model, &job.options, &references, cancelled) {
        Ok(batch) => batch,
        Err(error) => {
            let cancelled_error =
                error.starts_with("TASK_CANCELLED") || cancelled.load(Ordering::SeqCst);
            conn.execute(
                "UPDATE image_generation_jobs SET status=?1,error_json=?2,completed_at=?3 WHERE id=?4",
                params![if cancelled_error { "cancelled" } else { "failed" }, json!({"message": error, "retryable": !cancelled_error}).to_string(), now(), job_id],
            ).map_err(|e|e.to_string())?;
            return load_job(&conn, job_id);
        }
    };
    if cancelled.load(Ordering::SeqCst) {
        conn.execute(
            "UPDATE image_generation_jobs SET status='cancelled',error_json=?1,completed_at=?2 WHERE id=?3",
            params![json!({"message":"用户已取消，Provider 返回结果已丢弃","retryable":false}).to_string(),now(),job_id],
        ).map_err(|e|e.to_string())?;
        return load_job(&conn, job_id);
    }
    let candidate_dir = project.join("candidates").join("images").join(job_id);
    fs::create_dir_all(&candidate_dir).map_err(|e| format!("创建候选目录失败：{e}"))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (index, image) in batch.images.iter().enumerate() {
        let result_id = format!("{job_id}:{index}");
        let relative = PathBuf::from("candidates")
            .join("images")
            .join(job_id)
            .join(format!("{index}.{}", image.extension));
        let absolute = project.join(&relative);
        if !absolute.exists() {
            fs::write(&absolute, &image.bytes).map_err(|e| format!("保存候选图失败：{e}"))?;
        }
        let mut metadata = image.metadata.as_object().cloned().unwrap_or_default();
        metadata.insert("providerId".into(), Value::String(job.provider.clone()));
        metadata.insert("model".into(), Value::String(model.clone()));
        metadata.insert(
            "options".into(),
            serde_json::to_value(&job.options).map_err(|e| e.to_string())?,
        );
        tx.execute(
            "INSERT OR IGNORE INTO image_generation_results (id,job_id,file_path,metadata_json,sort_order,selection_state,created_at) VALUES (?1,?2,?3,?4,?5,'available',?6)",
            params![result_id,job_id,relative.to_string_lossy().replace('\\',"/"),Value::Object(metadata).to_string(),index as i64,now()],
        ).map_err(|e|e.to_string())?;
    }
    let status = if batch.error.is_some() {
        "partial"
    } else {
        "completed"
    };
    tx.execute(
        "UPDATE image_generation_jobs SET status=?1,model=?2,usage_json=?3,error_json=?4,completed_at=?5 WHERE id=?6 AND status='running'",
        params![status,model,batch.usage.to_string(),batch.error.map(|v|json!({"message":v,"retryable":true}).to_string()),now(),job_id],
    ).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    load_job(&conn, job_id)
}

fn record_job_failure(project: &Path, job_id: &str, error: &str) {
    if let Ok(conn) = open_database(project) {
        let _ = conn.execute(
            "UPDATE image_generation_jobs SET status='failed',error_json=?1,completed_at=?2 WHERE id=?3 AND status IN ('created','queued','running','interrupted')",
            params![json!({"message":error,"retryable":true}).to_string(),now(),job_id],
        );
    }
}

fn provider_for(
    app_data_dir: &Path,
    provider_id: &str,
) -> AppResult<Box<dyn ImageGenerationProvider>> {
    let config = load_provider(app_data_dir, provider_id)?;
    match config.provider_type.as_str() {
        "mock" => Ok(Box::new(MockProvider)),
        "openai_compatible" => Ok(Box::new(OpenAiCompatibleProvider {
            secret: provider_secret(provider_id)?,
            config,
        })),
        _ => Err("PROVIDER_NOT_CONFIGURED: 未知 Provider 类型".into()),
    }
}

fn current_prompt(conn: &Connection, target_type: &str, target_id: &str) -> AppResult<String> {
    match target_type {
        "assetRequirement" => conn.query_row(
            "SELECT prompt_draft FROM asset_requirements WHERE id=?1",
            [target_id],
            |r| r.get(0),
        ),
        "keyframe" => conn.query_row(
            "SELECT prompt_draft FROM keyframes WHERE id=?1",
            [target_id],
            |r| r.get(0),
        ),
        _ => return Err("TOOL_ARGUMENT_INVALID: 未知生图目标".into()),
    }
    .map_err(|_| "OBJECT_NOT_FOUND: 生图目标不存在".into())
}

fn formal_target(
    conn: &Connection,
    job: &ImageJob,
) -> AppResult<(String, String, Vec<MutationRequest>)> {
    let formal_object_id = new_id();
    match job.target_type.as_str() {
        "assetRequirement" => {
            let (asset_id, asset_type): (String,String) = conn.query_row(
                "SELECT asset.id,asset.type FROM asset_requirements requirement JOIN assets asset ON asset.id=requirement.asset_id WHERE requirement.id=?1",
                [&job.target_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"OBJECT_NOT_FOUND: 资产需求或所属资产不存在".to_string())?;
            let directory = match asset_type.as_str() {
                "character" => "assets/characters",
                "location" => "assets/locations",
                "prop" => "assets/props",
                _ => return Err("资产类型无效".into()),
            };
            let sort_order: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM asset_media WHERE asset_id=?1",
                    [&asset_id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let is_primary = i64::from(sort_order == 0);
            Ok((
                directory.into(),
                formal_object_id.clone(),
                vec![
                    mutation(
                        "create",
                        "assetMedia",
                        Some(formal_object_id.clone()),
                        json!({"asset_id":asset_id,"media_type":"image","label":"生成候选","sort_order":sort_order,"is_primary":is_primary,"source_type":"generated"}),
                    ),
                    mutation(
                        "create",
                        "assetMediaRequirement",
                        None,
                        json!({"asset_media_id":formal_object_id,"asset_requirement_id":job.target_id}),
                    ),
                ],
            ))
        }
        "keyframe" => Ok((
            "keyframes".into(),
            job.target_id.clone(),
            vec![mutation(
                "patch",
                "keyframe",
                Some(job.target_id.clone()),
                json!({"status":"ready"}),
            )],
        )),
        _ => Err("TOOL_ARGUMENT_INVALID: 未知生图目标".into()),
    }
}

fn mutation(
    action: &str,
    entity_type: &str,
    object_id: Option<String>,
    values: Value,
) -> MutationRequest {
    MutationRequest {
        action: action.into(),
        entity_type: entity_type.into(),
        object_id,
        values: values.as_object().cloned().unwrap_or_default(),
        change_set_id: None,
        change_set_name: None,
        source_type: Some("image_generation".into()),
        source_id: None,
    }
}

fn select_result(project: &Path, result_id: &str) -> AppResult<SelectImageResult> {
    let mut conn = open_database(project)?;
    let (job_id, candidate_path, state): (String, String, String) = conn
        .query_row(
            "SELECT job_id,file_path,selection_state FROM image_generation_results WHERE id=?1",
            [result_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "IMAGE_RESULT_MISSING: 候选图不存在".to_string())?;
    let job = load_job(&conn, &job_id)?;
    if state == "selected" {
        let metadata: String = conn
            .query_row(
                "SELECT metadata_json FROM image_generation_results WHERE id=?1",
                [result_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&metadata).unwrap_or_else(|_| json!({}));
        return Ok(SelectImageResult {
            formal_path: value
                .get("formalPath")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            formal_object_id: value
                .get("formalObjectId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            revision: conn
                .query_row("SELECT revision FROM projects LIMIT 1", [], |r| r.get(0))
                .map_err(|e| e.to_string())?,
        });
    }
    if state != "available" || !["completed", "partial"].contains(&job.status.as_str()) {
        return Err("IMAGE_RESULT_MISSING: 当前候选不可设为正式".into());
    }
    if current_prompt(&conn, &job.target_type, &job.target_id)?.trim() != job.prompt.trim() {
        return Err("REVISION_STALE: 目标提示词已变化，请重新生成".into());
    }
    let candidate = project.join(&candidate_path);
    if !candidate.is_file() {
        return Err("IMAGE_RESULT_MISSING: 候选文件不存在".into());
    }
    let extension = candidate
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("png");
    let (directory, formal_object_id, mut mutations) = formal_target(&conn, &job)?;
    let formal_relative = PathBuf::from(directory)
        .join(format!("{}.{}", new_id(), extension))
        .to_string_lossy()
        .replace('\\', "/");
    fs::copy(&candidate, project.join(&formal_relative))
        .map_err(|e| format!("复制正式图片失败：{e}"))?;
    mutations[0]
        .values
        .insert("file_path".into(), Value::String(formal_relative.clone()));
    let transaction_result = (|| -> AppResult<i64> {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let result = execute_mutations_in_transaction(
            &tx,
            mutations,
            None,
            Some("选择生图候选为正式图片".into()),
            Some("image_generation".into()),
            Some(job.id.clone()),
        )?;
        let existing_metadata: String = tx
            .query_row(
                "SELECT metadata_json FROM image_generation_results WHERE id=?1",
                [result_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut metadata: Map<String, Value> = serde_json::from_str::<Value>(&existing_metadata)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        metadata.insert("formalPath".into(), Value::String(formal_relative.clone()));
        metadata.insert(
            "formalObjectId".into(),
            Value::String(formal_object_id.clone()),
        );
        tx.execute("UPDATE image_generation_results SET selection_state='archived' WHERE job_id=?1 AND selection_state='selected'",[&job.id]).map_err(|e|e.to_string())?;
        tx.execute("UPDATE image_generation_results SET selection_state='selected',metadata_json=?1 WHERE id=?2",params![Value::Object(metadata).to_string(),result_id]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result.revision)
    })();
    let revision = match transaction_result {
        Ok(revision) => revision,
        Err(error) => {
            let _ = fs::remove_file(project.join(&formal_relative));
            return Err(error);
        }
    };
    Ok(SelectImageResult {
        formal_path: formal_relative,
        formal_object_id,
        revision,
    })
}

fn update_result_state(project: &Path, result_id: &str, state: &str) -> AppResult<ImageResult> {
    if !["rejected", "archived", "deleted"].contains(&state) {
        return Err("TOOL_ARGUMENT_INVALID: 候选状态无效".into());
    }
    let conn = open_database(project)?;
    let (job_id, file_path, current): (String, String, String) = conn
        .query_row(
            "SELECT job_id,file_path,selection_state FROM image_generation_results WHERE id=?1",
            [result_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| "IMAGE_RESULT_MISSING: 候选图不存在".to_string())?;
    if current == "selected" {
        return Err("TOOL_ARGUMENT_INVALID: 正式图片不能作为候选删除或拒绝".into());
    }
    conn.execute(
        "UPDATE image_generation_results SET selection_state=?1 WHERE id=?2",
        params![state, result_id],
    )
    .map_err(|e| e.to_string())?;
    if state == "deleted" {
        let _ = fs::remove_file(project.join(file_path));
    }
    load_job(&conn, &job_id)?
        .results
        .into_iter()
        .find(|result| result.id == result_id)
        .ok_or_else(|| "IMAGE_RESULT_MISSING: 候选图不存在".into())
}

#[tauri::command]
pub fn image_generate(
    app: tauri::AppHandle,
    state: tauri::State<'_, ImageGenerationState>,
    project_path: String,
    input: GenerateImageInput,
) -> AppResult<ImageJob> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if load_feature_flags(&app_data)?.get("image_generation") != Some(&true) {
        return Err("静态生图系统尚未启用".into());
    }
    let project = PathBuf::from(project_path);
    let config = load_provider(&app_data, &input.provider_id)?;
    if !input.reference_images.is_empty()
        && (!config.allow_image_upload
            || !config
                .capabilities
                .get("referenceImages")
                .and_then(Value::as_bool)
                .unwrap_or(false))
    {
        return Err("PROVIDER_CAPABILITY_MISSING: 当前 Provider 不允许上传参考图".into());
    }
    let job = create_job(&project, &input)?;
    if job.status != "queued" {
        return Ok(job);
    }
    let flag = Arc::new(AtomicBool::new(false));
    state
        .cancellations
        .lock()
        .map_err(|_| "任务状态锁损坏")?
        .insert(job.id.clone(), flag.clone());
    let state_copy = state.inner().clone();
    let job_id = job.id.clone();
    std::thread::spawn(move || {
        let result = provider_for(&app_data, &input.provider_id)
            .and_then(|provider| run_job(&app_data, &project, &job_id, provider.as_ref(), &flag));
        if let Err(error) = result {
            record_job_failure(&project, &job_id, &error);
        }
        state_copy
            .cancellations
            .lock()
            .ok()
            .map(|mut v| v.remove(&job_id));
    });
    Ok(job)
}

#[tauri::command]
pub fn image_get_job(project_path: String, job_id: String) -> AppResult<ImageJob> {
    load_job(&open_database(Path::new(&project_path))?, &job_id)
}

#[tauri::command]
pub fn image_list_jobs(
    project_path: String,
    target_type: String,
    target_id: String,
) -> AppResult<Vec<ImageJob>> {
    let conn = open_database(Path::new(&project_path))?;
    let mut stmt=conn.prepare("SELECT id FROM image_generation_jobs WHERE target_type=?1 AND target_id=?2 ORDER BY created_at DESC").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map(params![target_type, target_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_job(&conn, id)).collect()
}

#[tauri::command]
pub fn image_cancel(
    state: tauri::State<'_, ImageGenerationState>,
    project_path: String,
    job_id: String,
) -> AppResult<ImageJob> {
    if let Some(flag) = state
        .cancellations
        .lock()
        .map_err(|_| "任务状态锁损坏")?
        .get(&job_id)
    {
        flag.store(true, Ordering::SeqCst);
    }
    let conn = open_database(Path::new(&project_path))?;
    conn.execute("UPDATE image_generation_jobs SET status='cancelled',error_json=?1,completed_at=?2 WHERE id=?3 AND status IN ('created','queued','running')",params![json!({"message":"用户已取消","retryable":false}).to_string(),now(),job_id]).map_err(|e|e.to_string())?;
    load_job(&conn, &job_id)
}

#[tauri::command]
pub fn image_select_result(
    project_path: String,
    result_id: String,
) -> AppResult<SelectImageResult> {
    select_result(Path::new(&project_path), &result_id)
}

#[tauri::command]
pub fn image_update_result_state(
    project_path: String,
    result_id: String,
    selection_state: String,
) -> AppResult<ImageResult> {
    update_result_state(Path::new(&project_path), &result_id, &selection_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_database;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn setup_asset() -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "生图测试", "short").unwrap();
        fs::create_dir_all(temp.path().join("assets/characters")).unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute(
            "INSERT INTO assets (id,project_id,type,name,created_at,updated_at) VALUES ('asset',?1,'character','角色',?2,?2)",
            params![project.id, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO asset_requirements (id,asset_id,asset_type,prompt_draft,created_at,updated_at) VALUES ('requirement','asset','character','冷峻侦探，电影光影',?1,?1)",
            [timestamp],
        )
        .unwrap();
        (temp, "requirement".into())
    }

    fn input(id: &str, target_id: &str, mode: Option<&str>) -> GenerateImageInput {
        GenerateImageInput {
            request_id: id.into(),
            target_type: "assetRequirement".into(),
            target_id: target_id.into(),
            provider_id: "mock-provider".into(),
            model: Some("mock-image-1".into()),
            prompt: "冷峻侦探，电影光影".into(),
            reference_images: Vec::new(),
            options: ImageOptions {
                size: Some("1024x1024".into()),
                quality: Some("standard".into()),
                count: Some(2),
                background: Some("auto".into()),
                mock_mode: mode.map(str::to_string),
            },
        }
    }

    fn run_mock(project: &Path, job_id: &str) -> ImageJob {
        run_job(
            project,
            project,
            job_id,
            &MockProvider,
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    #[test]
    fn openai_compatible_reference_request_uses_multipart_edits_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (body_tx, body_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            let header_end = loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap();
            while request.len() - header_end < content_length {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
            }
            body_tx.send(request).unwrap();
            let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
            let body = format!(r#"{{"data":[{{"b64_json":"{png}"}}]}}"#);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let provider = OpenAiCompatibleProvider {
            config: ProviderConfig {
                id: "local".into(),
                provider_type: "openai_compatible".into(),
                display_name: "Local".into(),
                base_url: format!("http://{address}/v1"),
                default_model: "gpt-image-test".into(),
                capabilities: json!({"referenceImages":true}),
                timeout_seconds: 10,
                max_concurrency: 1,
                allow_image_upload: true,
                status: "configured".into(),
                has_secret: true,
                created_at: now(),
                updated_at: now(),
            },
            secret: "test-secret".into(),
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        let result = provider
            .generate(
                "保持角色一致",
                "gpt-image-test",
                &ImageOptions {
                    size: Some("1024x1024".into()),
                    quality: Some("high".into()),
                    count: Some(1),
                    background: Some("auto".into()),
                    mock_mode: None,
                },
                &[ReferenceImage {
                    bytes,
                    file_name: "hero.png".into(),
                    mime_type: "image/png",
                }],
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(result.images.len(), 1);
        let request = String::from_utf8_lossy(&body_rx.recv().unwrap()).to_string();
        assert!(request.starts_with("POST /v1/images/edits "));
        assert!(request.contains("name=\"image\""));
        assert!(request.contains("filename=\"hero.png\""));
        assert!(request.contains("保持角色一致"));
    }

    #[test]
    fn candidates_stay_non_formal_until_explicit_selection() {
        let (temp, target_id) = setup_asset();
        let created = create_job(temp.path(), &input("success", &target_id, None)).unwrap();
        assert_eq!(created.status, "queued");

        let completed = run_mock(temp.path(), &created.id);
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.results.len(), 2);
        assert!(completed
            .results
            .iter()
            .all(|result| result.selection_state == "available"
                && temp.path().join(&result.file_path).is_file()));
        assert_eq!(
            crate::commands::cleanup_project_media(temp.path().to_string_lossy().to_string())
                .unwrap(),
            0
        );
        assert_eq!(
            open_database(temp.path())
                .unwrap()
                .query_row("SELECT COUNT(*) FROM asset_media", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );

        let selected = select_result(temp.path(), &completed.results[0].id).unwrap();
        assert!(temp.path().join(&selected.formal_path).is_file());
        let selected_again = select_result(temp.path(), &completed.results[0].id).unwrap();
        assert_eq!(selected.formal_path, selected_again.formal_path);
        let conn = open_database(temp.path()).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM asset_media", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM asset_media_requirements", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }

    #[test]
    fn partial_failure_and_cancel_never_write_formal_facts() {
        for (id, mode, expected) in [
            ("partial", "partial", "partial"),
            ("failed", "fail", "failed"),
            ("cancelled", "cancel", "cancelled"),
        ] {
            let (temp, target_id) = setup_asset();
            create_job(temp.path(), &input(id, &target_id, Some(mode))).unwrap();
            let job = run_mock(temp.path(), id);
            assert_eq!(job.status, expected);
            let conn = open_database(temp.path()).unwrap();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM asset_media", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn terminal_jobs_are_idempotent_and_stale_prompt_blocks_selection() {
        let (temp, target_id) = setup_asset();
        let request = input("idempotent", &target_id, None);
        create_job(temp.path(), &request).unwrap();
        let first = run_mock(temp.path(), &request.request_id);
        let second = run_mock(temp.path(), &request.request_id);
        assert_eq!(first.results.len(), second.results.len());

        open_database(temp.path())
            .unwrap()
            .execute(
                "UPDATE asset_requirements SET prompt_draft='提示词已修改' WHERE id=?1",
                [&target_id],
            )
            .unwrap();
        assert!(select_result(temp.path(), &first.results[0].id)
            .unwrap_err()
            .contains("REVISION_STALE"));
    }

    #[test]
    fn references_must_be_explicit_or_formal_and_candidate_states_are_managed() {
        let (temp, target_id) = setup_asset();
        fs::create_dir_all(temp.path().join("references")).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        fs::write(temp.path().join("references/style.png"), png).unwrap();
        let mut valid = input("reference", &target_id, None);
        valid.reference_images = vec!["references/style.png".into()];
        assert_eq!(
            create_job(temp.path(), &valid)
                .unwrap()
                .reference_images
                .len(),
            1
        );

        let mut invalid = input("bad-reference", &target_id, None);
        invalid.reference_images = vec!["candidates/untrusted.png".into()];
        assert!(create_job(temp.path(), &invalid)
            .unwrap_err()
            .contains("参考图必须来自正式资产"));

        let completed = run_mock(temp.path(), "reference");
        let result = &completed.results[0];
        let candidate = temp.path().join(&result.file_path);
        assert!(candidate.is_file());
        let deleted = update_result_state(temp.path(), &result.id, "deleted").unwrap();
        assert_eq!(deleted.selection_state, "deleted");
        assert!(!candidate.exists());
    }

    #[test]
    fn keyframe_candidate_requires_selection_before_it_becomes_ready() {
        let temp = tempfile::tempdir().unwrap();
        let project = init_database(temp.path(), "关键帧生图", "short").unwrap();
        fs::create_dir_all(temp.path().join("keyframes")).unwrap();
        let conn = open_database(temp.path()).unwrap();
        let timestamp = now();
        conn.execute("INSERT INTO content_units (id,project_id,type,name,created_at,updated_at) VALUES ('unit',?1,'short','正片',?2,?2)",params![project.id,timestamp]).unwrap();
        conn.execute("INSERT INTO scripts (id,content_unit_id,title,created_at,updated_at) VALUES ('script','unit','正片',?1,?1)",[&timestamp]).unwrap();
        conn.execute("INSERT INTO scenes (id,script_id,title,created_at,updated_at) VALUES ('scene','script','场景',?1,?1)",[&timestamp]).unwrap();
        conn.execute("INSERT INTO shots (id,scene_id,title,created_at,updated_at) VALUES ('shot','scene','镜头',?1,?1)",[&timestamp]).unwrap();
        conn.execute("INSERT INTO keyframes (id,shot_id,prompt_draft,status,created_at,updated_at) VALUES ('frame','shot','电影感关键帧','planned',?1,?1)",[timestamp]).unwrap();
        drop(conn);

        let mut request = input("keyframe-job", "frame", None);
        request.target_type = "keyframe".into();
        request.prompt = "电影感关键帧".into();
        create_job(temp.path(), &request).unwrap();
        let completed = run_mock(temp.path(), &request.request_id);
        let before: (Option<String>, String) = open_database(temp.path())
            .unwrap()
            .query_row(
                "SELECT file_path,status FROM keyframes WHERE id='frame'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, (None, "planned".into()));

        let selected = select_result(temp.path(), &completed.results[0].id).unwrap();
        let after: (Option<String>, String) = open_database(temp.path())
            .unwrap()
            .query_row(
                "SELECT file_path,status FROM keyframes WHERE id='frame'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(after, (Some(selected.formal_path), "ready".into()));
    }
}
