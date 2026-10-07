use crate::names::named_enum;

named_enum! {
    /// The canonical operation catalog. The name is identical on the wire, in
    /// the database, as the bridge method and as the runtime method-table key.
    pub enum Operation {
        Init = "init",
        Discover = "discover",
        Doctor = "doctor",
        Render = "render",
        Still = "still",
        Frame = "frame",
        ContactSheet = "contact_sheet",
        Qa = "qa",
        Diagnose = "diagnose",
        ValidateMath = "validate_math",
        Captions = "captions",
        Ingest = "ingest",
        Export = "export",
    }
}

impl Operation {
    /// Direct operations run synchronously through the bridge without a job row.
    pub fn is_direct(self) -> bool {
        matches!(self, Self::Init | Self::Discover)
    }

    pub fn cacheable(self) -> bool {
        matches!(self, Self::Discover | Self::Render | Self::Still)
    }

    pub fn http_allowed(self) -> bool {
        !self.is_direct() && self != Self::Ingest
    }

    pub fn job_operations() -> impl Iterator<Item = Operation> {
        Self::ALL
            .iter()
            .copied()
            .filter(|operation| !operation.is_direct())
    }
}

named_enum! {
    pub enum JobStatus {
        Queued = "queued",
        Running = "running",
        Succeeded = "succeeded",
        Failed = "failed",
        Cancelled = "cancelled",
    }
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

named_enum! {
    /// Which frontend submitted a job; `engine` marks jobs the engine starts itself.
    pub enum JobOrigin {
        Http = "http",
        Mcp = "mcp",
        Cli = "cli",
        Engine = "engine",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn serde_display_and_from_str_agree_for_every_operation() {
        for operation in Operation::ALL {
            let wire = serde_json::to_value(operation).unwrap();
            assert_eq!(wire, serde_json::Value::String(operation.to_string()));
            assert_eq!(Operation::from_str(&operation.to_string()), Ok(*operation));
        }
        assert_eq!(Operation::ALL.len(), 13);
    }

    #[test]
    fn retired_spellings_are_rejected() {
        for retired in [
            "scaffold",
            "debug",
            "math_validate",
            "preview",
            "inspect",
            "contact-sheet",
        ] {
            assert!(Operation::from_str(retired).is_err(), "{retired}");
        }
    }

    #[test]
    fn execution_classes_follow_the_catalog() {
        let cacheable: Vec<_> = Operation::ALL.iter().filter(|op| op.cacheable()).collect();
        assert_eq!(
            cacheable,
            [&Operation::Discover, &Operation::Render, &Operation::Still]
        );
        let http: Vec<_> = Operation::ALL
            .iter()
            .filter(|op| op.http_allowed())
            .map(|op| op.as_str())
            .collect();
        assert_eq!(
            http,
            [
                "doctor",
                "render",
                "still",
                "frame",
                "contact_sheet",
                "qa",
                "diagnose",
                "validate_math",
                "captions",
                "export"
            ]
        );
    }
}
