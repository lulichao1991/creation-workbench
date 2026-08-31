mod agent_application;
mod agent_runtime;
mod app_database;
mod commands;
mod context;
mod database;
mod mutation;
mod permission;
mod story_structure;

use agent_application::{
    agent_create_session, agent_get_task, agent_list_experts, agent_list_messages,
    agent_resolve_intent, agent_send_message,
};
use agent_runtime::{
    agent_cancel_task, agent_get_task_state, agent_runtime_send_input,
    agent_runtime_start_readonly, RuntimeState,
};
use app_database::{get_feature_flags, set_feature_flag};
use commands::{
    cleanup_project_media, copy_project, create_project, delete_project, get_default_workspace,
    import_project_file, list_projects, load_project_state, open_project, read_project_media,
};
use context::{context_build, context_search};
use mutation::{
    apply_batch_mutation, apply_mutation, create_snapshot, list_history, restore_snapshot,
    undo_change_set,
};
use permission::{
    card_create, card_get, card_list, card_resolve, patch_apply, patch_get, patch_propose,
    patch_reject,
};
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
            app_database::initialize_app_database(&app_data_dir)?;
            app.manage(RuntimeState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_feature_flags,
            set_feature_flag,
            agent_list_experts,
            agent_resolve_intent,
            agent_create_session,
            agent_send_message,
            agent_get_task,
            agent_list_messages,
            agent_runtime_start_readonly,
            agent_runtime_send_input,
            agent_cancel_task,
            agent_get_task_state,
            context_build,
            context_search,
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
            load_project_state,
            import_project_file,
            read_project_media,
            cleanup_project_media,
            apply_mutation,
            apply_batch_mutation,
            list_history,
            undo_change_set,
            create_snapshot,
            restore_snapshot,
            graph_layout_save,
            graph_layout_reset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
