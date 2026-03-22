use crate::state::*;

/// Events sent FROM musicians/conductor TO the orchestra main loop.
#[derive(Debug, Clone)]
pub enum OrchestraEvent {
    // Musician events
    MusicianOutput {
        musician_id: String,
        line: String,
    },
    MusicianToolUse {
        musician_id: String,
        tool_name: String,
        tool_input: Option<serde_json::Value>,
    },
    MusicianStatusChange {
        musician_id: String,
        state: MusicianState,
    },
    MusicianRateLimit {
        musician_id: String,
        event: ClaudeEvent,
    },
    MusicianComplete {
        musician_id: String,
        result: TaskResult,
    },

    // Conductor agent events (during planning)
    ConductorOutput(String),
    ConductorToolUse {
        tool_name: String,
        tool_input: Option<serde_json::Value>,
    },

    // Insight generated
    InsightGenerated(Insight),

    // Error
    Error(String),
}

/// Actions sent FROM the TUI TO the orchestra.
#[derive(Debug, Clone)]
pub enum UserAction {
    Quit,
    FocusNext,
    FocusPrev,
    ToggleFocusView,
    SubmitGuidance {
        text: String,
        images: Option<Vec<String>>,
    },
    ApprovePlan,
    RejectPlan(String),
    RefinePlan {
        text: String,
        images: Option<Vec<String>>,
    },
    Resize { width: u16, height: u16 },
    ScrollUp,
    ScrollDown,
    ToggleHelp,
}
