use std::fmt;
use std::str::FromStr;

use crate::model::ORIGINAL;

/// A shot version on the command line: `<id>` or `<id>@<version>`, where the
/// version is `original` (the default) or a variant id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub shot: String,
    pub version: String,
}

impl Target {
    pub fn is_original(&self) -> bool {
        self.version == ORIGINAL
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (shot, version) = s.split_once('@').unwrap_or((s, ORIGINAL));
        if shot.is_empty() || version.is_empty() {
            return Err(format!(
                "invalid target {s:?}, expected <id> or <id>@<version>"
            ));
        }
        Ok(Target {
            shot: shot.to_string(),
            version: version.to_string(),
        })
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.shot, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_id_as_original() {
        let t: Target = "20261023-142530".parse().unwrap();
        assert_eq!(t.shot, "20261023-142530");
        assert!(t.is_original());
    }

    #[test]
    fn parses_variant() {
        let t: Target = "20261023-142530@v2".parse().unwrap();
        assert_eq!(t.version, "v2");
    }

    #[test]
    fn rejects_empty_parts() {
        assert!("@v1".parse::<Target>().is_err());
        assert!("abc@".parse::<Target>().is_err());
    }
}
