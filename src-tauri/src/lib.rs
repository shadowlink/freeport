mod commands;
mod discord;
mod error;
mod gamepad;
mod github;
mod install;
mod launch;
mod model;
mod mods;
mod platform;
mod store;
mod thumbs;

use commands::AppState;
use store::Paths;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK's DMABUF renderer crashes with a Wayland protocol error on many
    // Nvidia setups ("Error 71 dispatching to Wayland display"). Disabling it up
    // front—before the webview initializes—makes the app start reliably. Users
    // can still override by setting the variable themselves.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let paths = Paths::resolve().expect("no se pudieron preparar los directorios de datos");
    let client = reqwest::Client::builder()
        .user_agent("decompdeck")
        .build()
        .expect("no se pudo crear el cliente HTTP");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // `cover://` serves downscaled, cached cover thumbnails to <img>.
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let (client, paths) = {
                    let st = app.state::<AppState>();
                    (st.client.clone(), st.paths.clone())
                };
                let result = match thumbs::src_of(request.uri()) {
                    Some(url) => thumbs::get(&client, &paths, &url).await,
                    None => Err("missing src".to_string()),
                };
                let response = match result {
                    Ok(bytes) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", "image/jpeg")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(bytes),
                    Err(_) => tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new()),
                };
                if let Ok(r) = response {
                    responder.respond(r);
                }
            });
        })
        .manage(AppState {
            client,
            paths,
            mods_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            discord: discord::DiscordPresence::new(),
        })
        .setup(|app| {
            // Start reading controllers → emits `gamepad://input` for the TV UI.
            gamepad::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_platform,
            commands::fetch_wiki,
            commands::get_paths_info,
            commands::get_config,
            commands::set_config,
            commands::set_show_windows,
            commands::set_discord_app_id,
            commands::list_runners,
            commands::set_runner,
            commands::set_game_runner,
            commands::list_catalog,
            commands::refresh_catalog,
            commands::install_project,
            commands::uninstall_project,
            commands::set_rom,
            commands::launch_project,
            commands::check_updates,
            commands::list_mods,
            commands::install_mod,
            commands::installed_mods,
            commands::uninstall_mod,
            commands::is_tv_mode,
            commands::sunshine_status,
            commands::add_to_sunshine,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
