use std::{collections::HashSet, path::PathBuf};

use iced::Task;
use iced::widget::*;
use iced::widget::{button, column, container, row, text, text_input};
use serde::ser::SerializeStruct;

fn copy_dir(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VersionStage {
    Alpha,
    Beta,
    Release,
}

impl PartialOrd for VersionStage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VersionStage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use VersionStage::*;
        match (self, other) {
            (Alpha, Alpha) | (Beta, Beta) | (Release, Release) => std::cmp::Ordering::Equal,
            (Alpha, _) => std::cmp::Ordering::Less,
            (Beta, Alpha) => std::cmp::Ordering::Greater,
            (Beta, Release) => std::cmp::Ordering::Less,
            (Release, _) => std::cmp::Ordering::Greater,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    stage: VersionStage,
    build: u32,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(self.stage.cmp(&other.stage))
            .then(self.build.cmp(&other.build))
    }
}

impl std::str::FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().trim_start_matches('v');
        if s.is_empty() {
            return Err("Version string cannot be empty".to_string());
        }

        let parts: Vec<&str> = s.split('-').collect();
        let version_parts: Vec<&str> = parts[0].split('.').collect();

        if version_parts.len() != 3 {
            return Err("Version must be in the format major.minor.patch".to_string());
        }

        let major = version_parts[0]
            .parse::<u32>()
            .map_err(|_| "Invalid major version".to_string())?;
        let minor = version_parts[1]
            .parse::<u32>()
            .map_err(|_| "Invalid minor version".to_string())?;
        let patch = version_parts[2]
            .parse::<u32>()
            .map_err(|_| "Invalid patch version".to_string())?;

        let (stage, build) = if parts.len() > 1 {
            let stage_parts: Vec<&str> = parts[1].split('.').collect();
            let stage = match stage_parts[0] {
                "alpha" => VersionStage::Alpha,
                "beta" => VersionStage::Beta,
                "release" => VersionStage::Release,
                _ => return Err("Invalid version stage".to_string()),
            };
            let build = if stage_parts.len() > 1 {
                stage_parts[1]
                    .parse::<u32>()
                    .map_err(|_| "Invalid build number".to_string())?
            } else {
                0
            };
            (stage, build)
        } else {
            (VersionStage::Release, 0)
        };

        Ok(Version {
            major,
            minor,
            patch,
            stage,
            build,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match self.stage {
            VersionStage::Alpha => write!(f, "-alpha")?,
            VersionStage::Beta => write!(f, "-beta")?,
            VersionStage::Release => {}
        }
        if self.build > 0 {
            if matches!(self.stage, VersionStage::Release) {
                write!(f, "-release")?;
            }
            write!(f, ".{}", self.build)?;
        }
        Ok(())
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
            .map(|s| PathBuf::from(s))
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

        if !Self::check_sdl2() {
            #[cfg(target_os = "windows")]
            return Err(format!(
                "SDL2 library is not installed. Please put the correct SDL2.dll depending on your architecture into {} to run the game.",
                self.launcher_settings.game_dir.join("versions").display()
            ));
            #[cfg(target_os = "linux")]
            return Err("SDL2 library is not installed. Please install sdl2-compat using your package manager to run the game.".to_string());
            // The SDL2 framework would be added with the macOS app bundle anyways.
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

    async fn download_version(game_dir: PathBuf, version: Version) -> Result<Version, String> {
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

        let binary_data = download_response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read downloaded asset: {}", e))?;

        let exec_path = game_dir
            .join("versions")
            .join(if cfg!(target_os = "windows") {
                format!("{}.exe", version)
            } else if cfg!(target_os = "macos") {
                format!("{}.app", version)
            } else {
                version.to_string()
            });

        println!("Writing executable to {:?}", exec_path);

        std::fs::write(&exec_path, &binary_data)
            .map_err(|e| format!("Failed to write executable for version v{}: {}", version, e))?;

        // Are we on windows on x64? If so, install SDL2.dll if not present
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            let sdl2_path = game_dir.join("versions").join("SDL2.dll");
            if !sdl2_path.exists() {
                let sdl2_url = "https://www.libsdl.org/release/SDL2-2.0.14-win32-x64.zip";
                let sdl2_response = client
                    .get(sdl2_url)
                    .header("User-Agent", "mineplace3d-launcher")
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download SDL2.dll: {}", e))?;

                if !sdl2_response.status().is_success() {
                    return Err("Failed to download SDL2.dll".to_string());
                }

                let sdl2_data = sdl2_response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read SDL2.dll data: {}", e))?;

                let temp_zip_path = game_dir.join("versions").join("sdl2_temp.zip");
                std::fs::write(&temp_zip_path, &sdl2_data)
                    .map_err(|e| format!("Failed to write SDL2.dll zip file: {}", e))?;

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
            }
        }

        // Are we on windows on arm64? If so, install SDL2.dll if not present
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        {
            let sdl2_path = game_dir.join("versions").join("SDL2.dll");
            if !sdl2_path.exists() {
                // There aren't any official SDL2 builds for Windows ARM64, so we use an unofficial
                // one
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

                let sdl2_data = sdl2_response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read SDL2.dll data: {}", e))?;

                let temp_zip_path = game_dir.join("versions").join("sdl2_temp.zip");
                std::fs::write(&temp_zip_path, &sdl2_data)
                    .map_err(|e| format!("Failed to write SDL2.dll zip file: {}", e))?;

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
    fn check_sdl2(&self) -> bool {
        let sdl2_path = self
            .launcher_settings
            .game_dir
            .join("versions")
            .join("SDL2.dll");
        sdl2_path.exists()
    }

    #[cfg(target_os = "macos")]
    fn check_sdl2() -> bool {
        // On macOS, SDL2 is included in the app bundle, so we assume it's always present
        true
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
                        Task::perform(
                            Self::download_version(
                                self.launcher_settings.game_dir.clone(),
                                version,
                            ),
                            |res| match res {
                                Ok(v) => Message::VersionDownloaded(v),
                                Err(e) => Message::VersionDownloadFailed(e),
                            },
                        )
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
                println!("Version v{} downloaded successfully", version);
                Task::none()
            }
            Message::VersionDownloadFailed(error) => {
                eprintln!("Version download failed: {}", error);
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
        .style(if self.version_downloading {
            |theme: &Theme, _st| {
                let palette = theme.extended_palette();

                iced::widget::button::Style {
                    background: Some(palette.warning.weak.color.into()),
                    text_color: palette.warning.weak.text,
                    ..iced::widget::button::Style::default()
                }
            }
        } else {
            |theme: &Theme, _st| {
                let palette = theme.extended_palette();

                iced::widget::button::Style {
                    background: Some(palette.primary.base.color.into()),
                    text_color: palette.primary.base.text,
                    ..iced::widget::button::Style::default()
                }
            }
        });
        if !self.version_downloading {
            download_button =
                download_button.on_press(Message::Button(ButtonMessage::DownloadVersion));
        }

        let run_button = button("Run Version")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::RunVersion));

        let settings_button = button("Settings")
            .padding(10)
            .on_press(Message::Button(ButtonMessage::OpenSettings));

        let content = column![
            text("Mineplace3D Launcher").size(30),
            text("Installed Versions:").size(20),
            installed_versions,
            text("Select Version:").size(20),
            version_input,
            row![download_button, run_button, settings_button].spacing(10),
        ]
        .spacing(20)
        .padding(20);

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
        .run()
}
