use std::{collections::HashSet, path::PathBuf};

use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, StreamExt};
use iced::widget::*;
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Subscription, Task};
use serde::ser::SerializeStruct;

use crate::utils::{bytes_to_human_readable, copy_dir};
use crate::version::Version;

mod utils;
mod version;

#[derive(Debug, Clone)]
enum ButtonMessage {
    DownloadVersion,
    RunVersion,

    OpenSettings,
    ExitSettings,
    SaveSettings,
}

#[derive(Debug, Clone)]
enum InputMessage {
    VersionContentChanged(String),
    GameDirContentChanged(String),
}

#[derive(Debug, Clone)]
enum Message {
    Button(ButtonMessage),
    Input(InputMessage),
    VersionDownloaded(Version),
    VersionDownloadFailed(String),
    VersionDownloadUpdateReady(Sender<Message>),
    VersionDownloadUpdate(DownloadUpdate),
    VersionDownloadClear,
}

#[derive(Debug, Default, Clone)]
enum DownloadUpdate {
    Progress {
        progress: f32,
        /// Bytes per second
        speed: f32,
    },
    Finished,
    #[default]
    None,
}

impl DownloadUpdate {
    fn new(progress: f32, speed: f32) -> Self {
        Self::Progress { progress, speed }
    }
}

#[derive(Debug)]
struct LauncherSettings {
    game_dir: PathBuf,
}

impl serde::Serialize for LauncherSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("LauncherSettings", 1)?;
        state.serialize_field("game_dir", self.game_dir.to_str().unwrap())?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for LauncherSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = serde_json::Value::deserialize(deserializer)?;
        let game_dir = helper
            .get("game_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|data_dir| data_dir.join("mineplace3d")))
            .ok_or_else(|| serde::de::Error::custom("game_dir is required"))?;

        Ok(LauncherSettings { game_dir })
    }
}

enum View {
    Main,
    Settings,
}

struct Launcher {
    launcher_settings: LauncherSettings,
    versions: HashSet<Version>,
    input_version_content: String,
    input_game_dir_content: String,
    version_downloading: bool,
    version_download_update: DownloadUpdate,
    version_update_sender: Option<Sender<Message>>,
    view: View,
}

impl Launcher {
    fn new() -> Self {
        let launcher_settings_file = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mineplace3d-launcher")
            .join("launcher_settings.json");
        let launcher_settings = if launcher_settings_file.exists() {
            let config_data = std::fs::read_to_string(launcher_settings_file)
                .expect("Failed to read launcher configuration file");
            serde_json::from_str(&config_data).expect("Failed to parse launcher configuration file")
        } else {
            LauncherSettings {
                game_dir: dirs::data_dir()
                    .map(|data_dir| data_dir.join("mineplace3d"))
                    .expect("Failed to determine default game directory"),
            }
        };

        Self::setup_folder_structure(&launcher_settings.game_dir);

        let game_dir = launcher_settings.game_dir.clone();

        let mut launcher = Self {
            launcher_settings,
            versions: HashSet::new(),
            input_version_content: String::new(),
            input_game_dir_content: game_dir.to_string_lossy().to_string(),
            version_downloading: false,
            version_download_update: DownloadUpdate::default(),
            version_update_sender: None,
            view: View::Main,
        };

        launcher.load_versions();

        launcher
    }

    fn setup_folder_structure(game_dir: &PathBuf) {
        std::fs::create_dir_all(game_dir).expect("Failed to create game directory");
        std::fs::create_dir_all(game_dir.join("versions"))
            .expect("Failed to create versions directory");
    }

    fn get_file_from_game_dir(&self, relative_path: &str) -> Result<String, String> {
        let full_path = self.launcher_settings.game_dir.join(relative_path);
        if full_path.exists() {
            return std::fs::read_to_string(full_path)
                .map_err(|e| format!("Failed to read file {}: {}", relative_path, e));
        }
        Err(format!("File {} does not exist", relative_path))
    }

    fn load_versions(&mut self) {
        if let Ok(versions_data) = self.get_file_from_game_dir("versions/versions.json") {
            let versions: HashSet<String> =
                serde_json::from_str(&versions_data).expect("Failed to parse versions data");
            let versions_parsed: HashSet<Version> = versions
                .into_iter()
                .filter_map(|v_str| v_str.parse().ok())
                .collect();
            self.versions = versions_parsed;
        } else {
            self.versions = HashSet::new();
        }
    }

    fn run_version(&self, version: Version) -> Result<(), String> {
        if !self.versions.contains(&version) {
            return Err(format!("Version v{} is not available", version));
        }

        #[cfg(target_os = "windows")]
        if !Self::check_sdl2(&self.launcher_settings.game_dir) {
            return Err(format!(
                "SDL2 library is not installed. Please put the correct SDL2.dll depending on your architecture into {} to run the game.",
                self.launcher_settings.game_dir.join("versions").display()
            ));
        }
        #[cfg(target_os = "linux")]
        if !Self::check_sdl2() {
            return Err("SDL2 library is not installed. Please install sdl2-compat using your package manager to run the game.".to_string());
        }

        #[cfg(target_os = "linux")]
        let exec_path = self
            .launcher_settings
            .game_dir
            .join("versions")
            .join(version.to_string());

        #[cfg(target_os = "windows")]
        let exec_path = self
            .launcher_settings
            .game_dir
            .join("versions")
            .join(format!("{}.exe", version));

        #[cfg(target_os = "macos")]
        let exec_path = self
            .launcher_settings
            .game_dir
            .join("versions")
            .join(format!("{}.app", version));

        #[cfg(not(target_os = "macos"))]
        std::process::Command::new(&exec_path)
            .env("MINEPLACE3D_GAME_DIR", &self.launcher_settings.game_dir)
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to launch version v{} at {:?}: {}",
                    version, exec_path, e
                )
            })?;

        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg(&exec_path)
            .env("MINEPLACE3D_GAME_DIR", &self.launcher_settings.game_dir)
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to launch version v{} at {:?}: {}",
                    version, exec_path, e
                )
            })?;

        Ok(())
    }

    async fn download_version(
        game_dir: PathBuf,
        version: Version,
        mut progress_tx: Sender<Message>,
    ) -> Result<Version, String> {
        let release_url = format!(
            "https://api.github.com/repos/Muhtasim-Rasheed/mineplace3d/releases/tags/v{}",
            version
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&release_url)
            .header("User-Agent", "mineplace3d-launcher")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch release info: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Release for version v{} not found", version));
        }

        let release_info: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse release info: {}", e))?;

        let asset = release_info["assets"]
            .as_array()
            .and_then(|assets| {
                let platform = if cfg!(target_os = "windows") {
                    "windows"
                } else if cfg!(target_os = "linux") {
                    "linux"
                } else if cfg!(target_os = "macos") {
                    "macos"
                } else {
                    "unknown"
                };

                let arch = if cfg!(target_arch = "x86_64") {
                    "x86_64"
                } else if cfg!(target_arch = "aarch64") {
                    "aarch64"
                } else {
                    "unknown"
                };

                assets.iter().find(|asset| {
                    let name = format!(
                        "mineplace3d-{}-{}{}",
                        platform,
                        arch,
                        if cfg!(target_os = "windows") {
                            ".exe"
                        } else if cfg!(target_os = "macos") {
                            ".app"
                        } else {
                            ""
                        }
                    );
                    asset["name"].as_str() == Some(&name)
                })
            })
            .ok_or_else(|| format!("No suitable asset found for version v{}", version))?;

        let download_url = asset["browser_download_url"]
            .as_str()
            .ok_or_else(|| format!("Invalid asset download URL for version v{}", version))?;

        let download_response = client
            .get(download_url)
            .header("User-Agent", "mineplace3d-launcher")
            .send()
            .await
            .map_err(|e| format!("Failed to download asset: {}", e))?;

        if !download_response.status().is_success() {
            return Err(format!("Failed to download asset for version v{}", version));
        }

        let exec_path = game_dir
            .join("versions")
            .join(if cfg!(target_os = "windows") {
                format!("{}.exe", version)
            } else if cfg!(target_os = "macos") {
                format!("{}.app", version)
            } else {
                version.to_string()
            });

        let total_size = download_response.content_length();
        let mut downloaded = 0u64;
        let mut stream = download_response.bytes_stream();

        let mut last_tick = std::time::Instant::now();
        let mut downloaded_since_last = 0u64;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;

            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&exec_path)
                .and_then(|mut file| std::io::copy(&mut chunk.as_ref(), &mut file))
                .map_err(|e| format!("Write error: {}", e))?;

            let len = chunk.len() as u64;
            downloaded += len;
            downloaded_since_last += len;

            let elapsed = last_tick.elapsed();

            // Only update speed every 250ms
            if elapsed >= std::time::Duration::from_millis(250) {
                if let Some(total) = total_size {
                    let progress = downloaded as f32 / total as f32;
                    let speed = downloaded_since_last as f32 / elapsed.as_secs_f32();

                    let _ = progress_tx.try_send(Message::VersionDownloadUpdate(
                        DownloadUpdate::new(progress, speed),
                    ));
                }

                downloaded_since_last = 0;
                last_tick = std::time::Instant::now();
            }
        }

        let _ = progress_tx.try_send(Message::VersionDownloadUpdate(DownloadUpdate::Finished));

        // Are we on windows? If so, install SDL2.dll if not present
        #[cfg(target_os = "windows")]
        {
            let sdl2_path = game_dir.join("versions").join("SDL2.dll");
            if !sdl2_path.exists() {
                #[cfg(target_arch = "x86_64")]
                let sdl2_url = "https://www.libsdl.org/release/SDL2-2.32.10-win32-x64.zip";

                // SDL doesn't provide official arm64 builds, so we use a community build
                #[cfg(target_arch = "aarch64")]
                let sdl2_url = "https://www.github.com/mmozeiko/build-sdl2/releases/download/2025-12-28/SDL2-arm64-2025-12-28.zip";

                let sdl2_response = client
                    .get(sdl2_url)
                    .header("User-Agent", "mineplace3d-launcher")
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download SDL2.dll: {}", e))?;

                if !sdl2_response.status().is_success() {
                    return Err("Failed to download SDL2.dll".to_string());
                }

                let temp_zip_path = game_dir.join("versions").join("sdl2_temp.zip");

                let _ = progress_tx.try_send(Message::VersionDownloadUpdate(DownloadUpdate::new(
                    0.0, 0.0,
                )));

                let total_size = download_response.content_length();
                let mut downloaded = 0u64;
                let mut stream = download_response.bytes_stream();

                let mut last_tick = std::time::Instant::now();
                let mut downloaded_since_last = 0u64;

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;

                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&exec_path)
                        .and_then(|mut file| std::io::copy(&mut chunk.as_ref(), &mut file))
                        .map_err(|e| format!("Write error: {}", e))?;

                    let len = chunk.len() as u64;
                    downloaded += len;
                    downloaded_since_last += len;

                    let elapsed = last_tick.elapsed();

                    if elapsed >= std::time::Duration::from_millis(250) {
                        if let Some(total) = total_size {
                            let progress = downloaded as f32 / total as f32;
                            let speed = downloaded_since_last as f32 / elapsed.as_secs_f32();

                            let _ = progress_tx.try_send(Message::VersionDownloadUpdate(
                                DownloadUpdate::new(progress, speed),
                            ));
                        }

                        downloaded_since_last = 0;
                        last_tick = std::time::Instant::now();
                    }
                }

                let _ =
                    progress_tx.try_send(Message::VersionDownloadUpdate(DownloadUpdate::Finished));

                let mut zip = zip::ZipArchive::new(
                    std::fs::File::open(&temp_zip_path)
                        .map_err(|e| format!("Failed to open SDL2.dll zip file: {}", e))?,
                )
                .map_err(|e| format!("Failed to read SDL2.dll zip archive: {}", e))?;

                let mut sdl2_file = zip
                    .by_name("SDL2.dll")
                    .map_err(|e| format!("Failed to find SDL2.dll in zip archive: {}", e))?;

                let mut sdl2_out = std::fs::File::create(&sdl2_path)
                    .map_err(|e| format!("Failed to create SDL2.dll file: {}", e))?;
                std::io::copy(&mut sdl2_file, &mut sdl2_out)
                    .map_err(|e| format!("Failed to write SDL2.dll file: {}", e))?;

                std::fs::remove_file(&temp_zip_path)
                    .map_err(|e| format!("Failed to remove temporary SDL2.dll zip file: {}", e))?;

                _ = progress_tx.try_send(1.0);
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exec_path)
                .map_err(|e| format!("Failed to get metadata for {}: {}", exec_path.display(), e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exec_path, perms).map_err(|e| {
                format!(
                    "Failed to set permissions for {}: {}",
                    exec_path.display(),
                    e
                )
            })?;
        }

        Ok(version)
    }

    #[cfg(target_os = "linux")]
    fn check_sdl2() -> bool {
        use std::process::Command;

        let output = Command::new("ldconfig")
            .arg("-p")
            .output()
            .expect("Failed to execute ldconfig");

        if !output.status.success() {
            return false;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("libSDL2")
    }

    #[cfg(target_os = "windows")]
    fn check_sdl2(game_dir: &PathBuf) -> bool {
        let sdl2_path = game_dir.join("versions").join("SDL2.dll");
        sdl2_path.exists()
    }

    #[cfg(target_os = "macos")]
    fn check_sdl2() -> bool {
        // On macOS, SDL2 is included in the app bundle, so we assume it's always present
        true
    }

    /// Subscription to handle download progress updates.
    ///
    /// This subscription sets up a channel to receive progress updates from the
    /// asynchronous download task and simply forwards them as messages to the main application.
    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(|| {
            iced::stream::channel(100, async |mut sender| {
                let (tx, mut rx) = iced::futures::channel::mpsc::channel(100);

                // Notify the main application that the progress update channel is ready and give
                // it a way to send progress updates.
                sender
                    .send(Message::VersionDownloadUpdateReady(tx))
                    .await
                    .unwrap();

                // Forward progress updates from the download task to the main application
                loop {
                    if let Some(Message::VersionDownloadUpdate(update)) = rx.next().await {
                        sender
                            .send(Message::VersionDownloadUpdate(update))
                            .await
                            .unwrap();
                    }
                }
            })
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Button(button_msg) => match button_msg {
                ButtonMessage::DownloadVersion => {
                    if let Ok(version) = self.input_version_content.parse() {
                        if self.versions.contains(&version) {
                            return Task::none();
                        }

                        self.version_downloading = true;

                        let game_dir = self.launcher_settings.game_dir.clone();
                        if let Ok(version) = self.input_version_content.parse() {
                            let sender = self
                                .version_update_sender
                                .clone()
                                .expect("Download update sender not set");

                            let download_task = Task::perform(
                                Self::download_version(game_dir, version, sender),
                                |res| match res {
                                    Ok(v) => Message::VersionDownloaded(v),
                                    Err(e) => Message::VersionDownloadFailed(e),
                                },
                            );

                            download_task
                        } else {
                            eprintln!("Invalid version format: {}", self.input_version_content);
                            Task::none()
                        }
                    } else {
                        eprintln!("Invalid version format: {}", self.input_version_content);
                        Task::none()
                    }
                }
                ButtonMessage::RunVersion => {
                    if let Ok(version) = self.input_version_content.parse() {
                        self.run_version(version).unwrap_or_else(|e| {
                            eprintln!("Error running version: {}", e);
                        });
                    } else {
                        eprintln!("Invalid version format: {}", self.input_version_content);
                    }
                    Task::none()
                }
                ButtonMessage::OpenSettings => {
                    self.view = View::Settings;
                    Task::none()
                }
                ButtonMessage::ExitSettings => {
                    self.view = View::Main;
                    Task::none()
                }
                ButtonMessage::SaveSettings => {
                    let new_game_dir = PathBuf::from(&self.input_game_dir_content);

                    if new_game_dir != self.launcher_settings.game_dir {
                        if new_game_dir.exists() {
                            eprintln!("New game directory already exists: {:?}", new_game_dir);
                        } else {
                            std::fs::create_dir_all(&new_game_dir)
                                .expect("Failed to create new game directory");
                            if self.launcher_settings.game_dir.exists() {
                                copy_dir(&self.launcher_settings.game_dir, &new_game_dir)
                                    .expect("Failed to copy old game directory to new one");
                            }
                        }
                    }

                    self.launcher_settings.game_dir = new_game_dir;

                    let launcher_settings_file = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("mineplace3d-launcher")
                        .join("launcher_settings.json");
                    let settings_data = serde_json::to_string_pretty(&self.launcher_settings)
                        .expect("Failed to serialize launcher settings");
                    std::fs::create_dir_all(launcher_settings_file.parent().unwrap())
                        .expect("Failed to create launcher settings directory");
                    std::fs::write(launcher_settings_file, settings_data)
                        .expect("Failed to write launcher settings file");

                    self.view = View::Main;
                    self.input_game_dir_content = self
                        .launcher_settings
                        .game_dir
                        .to_string_lossy()
                        .to_string();
                    println!(
                        "Settings saved successfully. New game directory: {:?}",
                        self.launcher_settings.game_dir
                    );
                    self.load_versions();

                    Task::none()
                }
            },
            Message::Input(input_msg) => match input_msg {
                InputMessage::VersionContentChanged(new) => {
                    self.input_version_content = new;
                    Task::none()
                }
                InputMessage::GameDirContentChanged(new) => {
                    self.input_game_dir_content = new;
                    Task::none()
                }
            },
            Message::VersionDownloaded(version) => {
                self.versions.insert(version);
                self.version_downloading = false;
                let versions_str: HashSet<String> =
                    self.versions.iter().map(|v| v.to_string()).collect();
                let versions_data = serde_json::to_string_pretty(&versions_str)
                    .expect("Failed to serialize versions");
                let versions_file_path = self
                    .launcher_settings
                    .game_dir
                    .join("versions")
                    .join("versions.json");
                std::fs::write(versions_file_path, versions_data)
                    .expect("Failed to write versions file");
                Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    },
                    |_| Message::VersionDownloadClear,
                )
            }
            Message::VersionDownloadClear => {
                self.version_download_update = DownloadUpdate::default();
                Task::none()
            }
            Message::VersionDownloadFailed(error) => {
                eprintln!("Version download failed: {}", error);
                self.version_downloading = false;
                self.version_download_update = DownloadUpdate::default();
                Task::none()
            }
            Message::VersionDownloadUpdateReady(sender) => {
                self.version_update_sender = Some(sender);
                Task::none()
            }
            Message::VersionDownloadUpdate(update) => {
                self.version_download_update = update;
                Task::none()
            }
        }
    }

    fn main_view(&self) -> iced::Element<'_, Message> {
        let mut installed_versions = Column::new();
        let mut versions: Vec<Version> = self.versions.iter().copied().collect();
        versions.sort();
        versions.reverse();
        let mut dark = false;
        for version in versions {
            installed_versions = installed_versions.push(
                container(text(format!("v{}", version)).size(16))
                    .padding(5)
                    .width(iced::Length::Fill)
                    .style(if dark {
                        |theme: &Theme| {
                            let palette = theme.extended_palette();

                            iced::widget::container::Style {
                                background: Some(palette.success.weak.color.into()),
                                text_color: Some(palette.success.weak.text),
                                ..iced::widget::container::Style::default()
                            }
                        }
                    } else {
                        |theme: &Theme| {
                            let palette = theme.extended_palette();

                            iced::widget::container::Style {
                                background: Some(palette.success.base.color.into()),
                                text_color: Some(palette.success.base.text),
                                ..iced::widget::container::Style::default()
                            }
                        }
                    }),
            );
            dark = !dark;
        }

        let version_input = text_input(
            "Enter version (e.g., 0.3.0-alpha.1)",
            &self.input_version_content,
        )
        .on_input(|value| Message::Input(InputMessage::VersionContentChanged(value)))
        .padding(10)
        .size(20);

        let mut download_button = button(if self.version_downloading {
            "Downloading..."
        } else {
            "Download Version"
        })
        .padding(10)
        .style(|theme: &Theme, status: button::Status| {
            let palette = theme.extended_palette();

            match status {
                button::Status::Disabled => iced::widget::button::Style {
                    background: Some(palette.warning.weak.color.into()),
                    text_color: palette.warning.weak.text,
                    ..iced::widget::button::Style::default()
                },
                _ => iced::widget::button::Style {
                    background: Some(palette.primary.base.color.into()),
                    text_color: palette.primary.base.text,
                    ..iced::widget::button::Style::default()
                },
            }
        });

        if !self.version_downloading && !self.input_version_content.is_empty() {
            download_button =
                download_button.on_press(Message::Button(ButtonMessage::DownloadVersion));
        }

        match self.input_version_content.parse::<Version>() {
            Ok(version) if self.versions.contains(&version) || self.version_downloading => {
                download_button = download_button.on_press_maybe(None);
            }
            Err(_) => {
                download_button = download_button.on_press_maybe(None);
            }
            _ => {}
        }

        let run_button = button("Run Version")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::RunVersion));

        let settings_button = button("Settings")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::OpenSettings));

        let mut content = column![
            text("Mineplace3D Launcher").size(30),
            text("Installed Versions:").size(20),
            installed_versions,
            text("Select Version:").size(20),
            version_input,
            row![download_button, run_button, settings_button].spacing(10),
        ]
        .spacing(20)
        .padding(20);

        if let DownloadUpdate::Progress { progress, speed } = self.version_download_update {
            let progress_bar = iced::widget::progress_bar(0.0..=1.0, progress)
                .length(iced::Length::Fill)
                .girth(20);
            content = content.push(progress_bar);
            let progress_text = text(format!(
                "Download Progress: {:.2}%, Speed: {}/s",
                progress * 100.0,
                bytes_to_human_readable(speed),
            ))
            .size(16);
            content = content.push(progress_text);
        } else if let DownloadUpdate::Finished = self.version_download_update {
            let progress_bar = iced::widget::progress_bar(0.0..=1.0, 1.0)
                .length(iced::Length::Fill)
                .girth(20);
            content = content.push(progress_bar);
            let progress_text = text("Download Complete!").size(16);
            content = content.push(progress_text);
        }

        container(content).center(iced::Fill).into()
    }

    fn settings_view(&self) -> iced::Element<'_, Message> {
        let game_dir_input = text_input("Game Directory", &self.input_game_dir_content)
            .on_input(|value| Message::Input(InputMessage::GameDirContentChanged(value)))
            .padding(10)
            .size(20);

        let save_button = button("Save Settings")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::SaveSettings));

        let exit_button = button("Back")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::ExitSettings));

        let content = column![
            text("Launcher Settings").size(30),
            text("Game Directory:").size(20),
            game_dir_input,
            row![save_button, exit_button].spacing(10),
            text("Advanced").size(30),
            text!(
                "To manually change the game directory, edit the launcher_settings.json file located in {}.",
                dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("mineplace3d-launcher").display()
            ).size(16),
        ]
        .spacing(20)
        .padding(20);

        container(content).center(iced::Fill).into()
    }

    fn view(&self) -> iced::Element<'_, Message> {
        match self.view {
            View::Main => self.main_view(),
            View::Settings => self.settings_view(),
        }
    }
}

fn main() -> iced::Result {
    iced::application(Launcher::new, Launcher::update, Launcher::view)
        .theme(iced::theme::Theme::CatppuccinMocha)
        .default_font(iced::Font::MONOSPACE)
        .title("Mineplace3D Launcher")
        .subscription(Launcher::subscription)
        .run()
}
