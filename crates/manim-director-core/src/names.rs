use std::fmt;

/// Declares a fieldless enum whose serde name, `Display` and `FromStr` are one
/// and the same string, so the spelling cannot drift between the wire, the
/// database and the command line.
macro_rules! named_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::UnknownName;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|item| item.as_str() == value)
                    .ok_or_else(|| $crate::UnknownName {
                        kind: stringify!($name),
                        value: value.to_owned(),
                        allowed: Self::ALL.iter().map(|item| item.as_str()).collect(),
                    })
            }
        }
    };
}

pub(crate) use named_enum;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownName {
    pub kind: &'static str,
    pub value: String,
    pub allowed: Vec<&'static str>,
}

impl fmt::Display for UnknownName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown {} {:?}; expected one of {}",
            self.kind,
            self.value,
            self.allowed.join(", ")
        )
    }
}

impl std::error::Error for UnknownName {}
