#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub enum BuildArch {
    All,
    x86_64,
    x86,
    Arm,
    Armv6h,
    Armv7h,
    Arm64,
    Other(String),
}

impl From<&str> for BuildArch {
    fn from(s: &str) -> Self {
        match &s.to_lowercase()[..] {
            "all" | "any" | "noarch" => Self::All,
            "x86_64" | "amd64" => Self::x86_64,
            "i386" | "x86" => Self::x86,
            "armel" | "arm" => Self::Arm,
            "armv6hl" | "armv6h" => Self::Armv6h,
            "armv7hl" | "armv7h" | "armhf" => Self::Armv7h,
            "aarch64" | "arm64" => Self::Arm64,
            arch => Self::Other(arch.to_string()),
        }
    }
}

impl BuildArch {
    pub fn deb_name(&self) -> &str {
        use BuildArch::*;
        match &self {
            All => "all",
            x86_64 => "amd64",
            x86 => "i386",
            Arm => "armel",
            Armv6h => "armhf",
            Armv7h => "armhf",
            Arm64 => "arm64",
            Other(arch) => &arch,
        }
    }

    pub fn rpm_name(&self) -> &str {
        use BuildArch::*;
        match &self {
            All => "noarch",
            x86_64 => "x86_64",
            x86 => "i386",
            Arm => "armel",
            Armv6h => "armv6hl",
            Armv7h => "armv7hl",
            Arm64 => "aarch64",
            Other(arch) => &arch,
        }
    }

    pub fn pkg_name(&self) -> &str {
        use BuildArch::*;
        match &self {
            All => "any",
            x86_64 => "x86_64",
            x86 => "i386",
            Arm => "arm",
            Armv6h => "armv6h",
            Armv7h => "armv7h",
            Arm64 => "aarch64",
            Other(arch) => &arch,
        }
    }
}
