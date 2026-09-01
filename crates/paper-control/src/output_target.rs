#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputTarget {
    All,
    Named(String),
    Set(Vec<String>),
}

impl OutputTarget {
    pub fn from_arg(output: &str) -> Self {
        if output == "*" {
            Self::All
        } else if output.contains(',') {
            Self::Set(output.split(',').filter(|part| !part.is_empty()).map(String::from).collect())
        } else {
            Self::Named(output.to_string())
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Named(target) => target == name,
            Self::Set(targets) => targets.iter().any(|target| target == name),
        }
    }
}
