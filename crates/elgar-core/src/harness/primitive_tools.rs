//! Primitive tool metadata for model-requestable harness work.
//!
//! This registry is a table of contents. It describes what the model may
//! request, but it does not route user text and does not execute anything.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveToolId {
    Read,
    Ls,
    Find,
    Grep,
    Bash,
    Write,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveToolSideEffectLevel {
    None,
    ReadOnly,
    Shell,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimitiveTool {
    pub id: PrimitiveToolId,
    pub display_name: &'static str,
    pub description: &'static str,
    pub input_shape: &'static str,
    pub side_effect_level: PrimitiveToolSideEffectLevel,
    pub enabled_in_stage: bool,
    pub executable_in_stage: bool,
    pub requires_permission: bool,
    pub limits: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimitiveToolRegistry {
    tools: Vec<PrimitiveTool>,
}

impl PrimitiveToolRegistry {
    /// Build a registry from explicit metadata.
    ///
    /// This is mainly useful for focused parser tests and future staged
    /// manifests.
    pub fn new(tools: Vec<PrimitiveTool>) -> Self {
        Self { tools }
    }

    /// Return the current conservative primitive tool manifest.
    pub fn stage_3a() -> Self {
        Self {
            tools: vec![
                PrimitiveTool {
                    id: PrimitiveToolId::Read,
                    display_name: "Read",
                    description: "Read bounded UTF-8 contents from one file path.",
                    input_shape: r#"{"type":"structured_request","kind":"read","reason":"short reason","arguments":{"path":"path/from/user.ext"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::ReadOnly,
                    enabled_in_stage: true,
                    executable_in_stage: true,
                    requires_permission: false,
                    limits: &[
                        "requires arguments.path",
                        "relative paths resolve from the launch folder",
                        "absolute paths are allowed",
                        "does not read missing files, directories, symlinks, or binary files",
                        "file contents are byte-limited and may be truncated",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Ls,
                    display_name: "Ls",
                    description: "List one directory with bounded entries and counts.",
                    input_shape: r#"{"type":"structured_request","kind":"ls","reason":"short reason","arguments":{"path":"path/from/user"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::ReadOnly,
                    enabled_in_stage: true,
                    executable_in_stage: true,
                    requires_permission: false,
                    limits: &[
                        "requires arguments.path",
                        "relative paths resolve from the launch folder",
                        "absolute paths are allowed",
                        "does not read file contents",
                        "directory entries are bounded and may be truncated",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Find,
                    display_name: "Find",
                    description:
                        "Find file and directory paths by name pattern under one directory.",
                    input_shape: r#"{"type":"structured_request","kind":"find","reason":"short reason","arguments":{"path":".","pattern":"text"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::ReadOnly,
                    enabled_in_stage: true,
                    executable_in_stage: true,
                    requires_permission: false,
                    limits: &[
                        "requires arguments.pattern",
                        "relative paths resolve from the launch folder",
                        "skips noisy generated/cache/log/dependency folders",
                        "results are bounded and may be truncated",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Grep,
                    display_name: "Grep",
                    description: "Search text inside bounded UTF-8 files under one directory.",
                    input_shape: r#"{"type":"structured_request","kind":"grep","reason":"short reason","arguments":{"path":".","query":"text"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::ReadOnly,
                    enabled_in_stage: true,
                    executable_in_stage: true,
                    requires_permission: false,
                    limits: &[
                        "requires arguments.query",
                        "relative paths resolve from the launch folder",
                        "skips noisy generated/cache/log/dependency folders",
                        "results are bounded and may be truncated",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Bash,
                    display_name: "Bash",
                    description: "Run one shell command after policy approval.",
                    input_shape: r#"{"type":"structured_request","kind":"bash","reason":"short reason","arguments":{"command":"command text"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::Shell,
                    enabled_in_stage: true,
                    executable_in_stage: false,
                    requires_permission: true,
                    limits: &[
                        "declared primitive only in this stage",
                        "does not execute yet",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Write,
                    display_name: "Write",
                    description: "Create or overwrite one file after policy approval.",
                    input_shape: r#"{"type":"structured_request","kind":"write","reason":"short reason","arguments":{"path":"path/from/user.ext","content":"text"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::Write,
                    enabled_in_stage: true,
                    executable_in_stage: false,
                    requires_permission: true,
                    limits: &[
                        "declared primitive only in this stage",
                        "does not execute yet",
                    ],
                },
                PrimitiveTool {
                    id: PrimitiveToolId::Edit,
                    display_name: "Edit",
                    description: "Patch one existing file after policy approval.",
                    input_shape: r#"{"type":"structured_request","kind":"edit","reason":"short reason","arguments":{"path":"path/from/user.ext","patch":"unified patch or edit description"}}"#,
                    side_effect_level: PrimitiveToolSideEffectLevel::Write,
                    enabled_in_stage: true,
                    executable_in_stage: false,
                    requires_permission: true,
                    limits: &[
                        "declared primitive only in this stage",
                        "does not execute yet",
                    ],
                },
            ],
        }
    }

    /// Return every primitive tool known to this registry.
    pub fn tools(&self) -> &[PrimitiveTool] {
        &self.tools
    }

    /// Find a primitive tool by id.
    pub fn get(&self, id: PrimitiveToolId) -> Option<&PrimitiveTool> {
        self.tools.iter().find(|tool| tool.id == id)
    }

    /// Return whether an id is known and enabled in the current stage.
    pub fn enabled(&self, id: PrimitiveToolId) -> bool {
        self.get(id)
            .map(|tool| tool.enabled_in_stage)
            .unwrap_or(false)
    }
}

impl PrimitiveToolId {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "ls" => Some(Self::Ls),
            "find" => Some(Self::Find),
            "grep" => Some(Self::Grep),
            "bash" => Some(Self::Bash),
            "write" => Some(Self::Write),
            "edit" => Some(Self::Edit),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Ls => "ls",
            Self::Find => "find",
            Self::Grep => "grep",
            Self::Bash => "bash",
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }
}

impl PrimitiveToolSideEffectLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReadOnly => "read_only",
            Self::Shell => "shell",
            Self::Write => "write",
        }
    }
}
