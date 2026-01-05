#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VersionStage {
    Alpha,
    Beta,
    #[default]
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
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    stage: VersionStage,
    build: u32,
}

impl Default for Version {
    fn default() -> Self {
        Version {
            major: 0,
            minor: 2,
            patch: 2,
            stage: VersionStage::Release,
            build: 0,
        }
    }
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
            (VersionStage::default(), 0)
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
