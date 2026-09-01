mod agent_application;
mod agent_gateway;
mod agent_models;
mod agent_runtime;
mod app_database;
mod commands;
mod context;
mod database;
mod expert_team;
mod image_generation;
mod memory;
mod mutation;
mod permission;
mod prompt_compiler;
mod provider;
mod story_structure;

use agent_application::{
    agent_close_session, agent_create_session, agent_get_task, agent_list_experts,
    agent_list_messages, agent_list_sessions, agent_resolve_intent, agent_resume_session,
    agent_send_message,
};
use agent_models::{
    agent_model_settings_get, agent_model_settings_save, agent_provider_login,
    agent_provider_logout,
};
use agent_runtime::{
    agent_cancel_task, agent_get_task_state, agent_provider_test, agent_runtime_doctor,
    agent_runtime_follow_up, agent_runtime_send_input, agent_runtime_start_readonly, RuntimeState,
};
use app_database::{get_feature_flags, set_feature_flag};
use commands::{
    cleanup_project_media, copy_project, create_project, delete_project, export_production_package,
    get_default_workspace, import_project_file, list_projects, load_project_state, open_project,
    read_project_media,
};
use context::{context_build, context_search};
use expert_team::{
    expert_team_cancel, expert_team_confirm, expert_team_get, expert_team_list, expert_team_request,
};
use image_generation::{
    image_cancel, image_generate, image_get_job, image_list_jobs, image_list_recent_jobs,
    image_select_result, image_update_result_state, ImageGenerationState,
};
use memory::{memory_create, memory_invalidate, memory_list, memory_update};
use mutation::{
    apply_batch_mutation, apply_mutation, create_snapshot, get_snapshot, list_history,
    restore_snapshot, undo_change_set,
};
use permission::{
    card_create, card_get, card_list, card_resolve, patch_apply, patch_get, patch_propose,
    patch_reject,
};
use prompt_compiler::{
    prompt_compile, prompt_list_compilations, prompt_list_profiles, prompt_list_templates,
    prompt_save_profile, prompt_save_template, prompt_set_current,
};
use provider::{provider_delete, provider_list, provider_save, provider_test};
use story_structure::{graph_layout_reset, graph_layout_save};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = tauri::Manager::path(app)
                .app_data_dir()
                .map_err(|e| format!("读取应用数据目录失败：{e}"))?;
            let resource_dir = tauri::Manager::path(app)
                .resource_dir()
                .map_err(|e| format!("读取应用资源目录失败：{e}"))?;
            app_database::initialize_app_database(&app_data_dir)?;
            app.manage(RuntimeState::for_resource_dir(resource_dir));
            app.manage(ImageGenerationState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_feature_flags,
            set_feature_flag,
            agent_list_experts,
            agent_resolve_intent,
            agent_create_session,
            agent_list_sessions,
            agent_close_session,
            agent_resume_session,
            agent_send_message,
            agent_get_task,
            agent_list_messages,
            expert_team_request,
            expert_team_confirm,
            expert_team_get,
            expert_team_list,
            expert_team_cancel,
            agent_runtime_start_readonly,
            agent_runtime_doctor,
            agent_provider_test,
            agent_runtime_send_input,
            agent_runtime_follow_up,
            agent_cancel_task,
            agent_get_task_state,
            agent_model_settings_get,
            agent_model_settings_save,
            agent_provider_login,
            agent_provider_logout,
            context_build,
            context_search,
            memory_list,
            memory_create,
            memory_update,
            memory_invalidate,
            provider_list,
            provider_save,
            provider_delete,
            provider_test,
            prompt_list_profiles,
            prompt_save_profile,
            prompt_list_templates,
            prompt_save_template,
            prompt_compile,
            prompt_list_compilations,
            prompt_set_current,
            image_generate,
            image_get_job,
            image_list_jobs,
            image_list_recent_jobs,
            image_cancel,
            image_select_result,
            image_update_result_state,
            patch_propose,
            patch_get,
            patch_apply,
            patch_reject,
            card_create,
            card_get,
            card_list,
            card_resolve,
            get_default_workspace,
            list_projects,
            create_project,
            open_project,
            copy_project,
            delete_project,
            export_production_package,
            load_project_state,
            import_project_file,
            read_project_media,
            cleanup_project_media,
            apply_mutation,
            apply_batch_mutation,
            list_history,
            undo_change_set,
            create_snapshot,
            get_snapshot,
            restore_snapshot,
            graph_layout_save,
            graph_layout_reset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
