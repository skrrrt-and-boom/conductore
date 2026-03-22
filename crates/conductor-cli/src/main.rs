use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use conductor_bridge::{validate_claude_cli, validate_model};
use conductor_core::{orchestra::Orchestra, task_store::TaskStore, CoreError};
use conductor_types::OrchestraConfig;
use tracing::info;
use uuid::Uuid;

// ─── CLI Definition ─────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "conductor",
    about = "Multi-agent AI orchestrator",
    version,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Project directory path
    #[arg(short = 'p', long, default_value = ".")]
    project: String,

    /// Default musician count
    #[arg(short = 'm', long, default_value_t = 3)]
    musicians: usize,

    /// Model for Conductor agent
    #[arg(long, default_value = "opus")]
    conductor_model: String,

    /// Model for Musician agents
    #[arg(long, default_value = "sonnet")]
    musician_model: String,

    /// Max turns per musician
    #[arg(long, default_value_t = 30)]
    max_turns: u32,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a session in headless mode (non-interactive)
    Run {
        /// Project directory path
        #[arg(short = 'p', long)]
        project: String,

        /// Task description
        #[arg(short = 't', long)]
        task: String,

        /// Musician count
        #[arg(short = 'm', long, default_value_t = 3)]
        musicians: usize,

        /// Model for Conductor agent
        #[arg(long, default_value = "opus")]
        conductor_model: String,

        /// Model for Musician agents
        #[arg(long, default_value = "sonnet")]
        musician_model: String,

        /// Max turns per musician
        #[arg(long, default_value_t = 30)]
        max_turns: u32,

        /// Dry run (no actual API calls)
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Reference a previous session ID to build upon
        #[arg(long)]
        ref_session: Option<String>,
    },

    /// Resume a paused session
    Resume {
        /// Session ID to resume
        #[arg(short = 's', long)]
        session: String,
    },

    /// List all sessions
    List,

    /// Show session status
    Status {
        /// Session ID
        #[arg(short = 's', long)]
        session: String,
    },

    /// Remove session data
    Clean {
        /// Session ID to delete
        #[arg(short = 's', long)]
        session: Option<String>,

        /// Delete all sessions
        #[arg(long, default_value_t = false)]
        all: bool,

        /// Delete sessions older than N days
        #[arg(long)]
        older_than: Option<u64>,

        /// Keep only N most recent sessions
        #[arg(long)]
        keep: Option<usize>,
    },
}

// ─── Entry Point ────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: interactive mode
            let project = std::fs::canonicalize(&cli.project)
                .with_context(|| format!("invalid project path: {}", cli.project))?
                .to_string_lossy()
                .to_string();
            run_interactive(
                project,
                cli.musicians,
                cli.conductor_model,
                cli.musician_model,
                cli.max_turns,
            )
            .await
        }
        Some(Commands::Run {
            project,
            task,
            musicians,
            conductor_model,
            musician_model,
            max_turns,
            dry_run,
            ref_session,
        }) => {
            let project = std::fs::canonicalize(&project)
                .with_context(|| format!("invalid project path: {project}"))?
                .to_string_lossy()
                .to_string();
            run_headless(
                project,
                task,
                musicians,
                conductor_model,
                musician_model,
                max_turns,
                dry_run,
                ref_session,
            )
            .await
        }
        Some(Commands::Resume { session }) => resume_session(session).await,
        Some(Commands::List) => list_sessions().await,
        Some(Commands::Status { session }) => session_status(session).await,
        Some(Commands::Clean {
            session,
            all,
            older_than,
            keep,
        }) => clean_sessions(session, all, older_than, keep).await,
    }
}

// ─── Subcommand Implementations ─────────────────────────────────

async fn list_sessions() -> Result<()> {
    let sessions = TaskStore::list_sessions().await.context("failed to list sessions")?;
    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }
    println!("\nSessions:\n");
    for s in &sessions {
        let task_count = s.tasks.len();
        let completed = s.tasks.iter().filter(|t| t.status == conductor_types::TaskStatus::Completed).count();
        let cancelled = s.tasks.iter().filter(|t| t.status == conductor_types::TaskStatus::Cancelled).count();
        let cancelled_suffix = if cancelled > 0 {
            format!(" ({cancelled} cancelled)")
        } else {
            String::new()
        };
        let phase = format!("{:?}", s.phase);
        let desc = &s.config.task_description;
        let desc_short = if desc.len() > 50 { &desc[..50] } else { desc };
        println!(
            "  {}  {:<12}  {}/{} tasks{}  {}",
            s.id, phase, completed, task_count, cancelled_suffix, desc_short
        );
    }
    println!();
    Ok(())
}

async fn session_status(session_id: String) -> Result<()> {
    let resolved = TaskStore::resolve_id(&session_id)
        .await
        .with_context(|| format!("session '{session_id}' not found"))?;

    let store = TaskStore::new(&resolved);
    let session = store
        .load_session()
        .await
        .context("failed to load session")?
        .with_context(|| format!("session '{resolved}' not found"))?;

    println!("\nSession: {}", session.id);
    println!("Phase: {:?}", session.phase);
    println!("Task: {}", session.config.task_description);
    println!("Started: {}", session.started_at);
    println!("\nTasks:");
    for task in &session.tasks {
        let status = format!("{:?}", task.status);
        println!("  {}. [{:<12}] {}", task.index + 1, status, task.title);
    }
    println!();
    Ok(())
}

async fn clean_sessions(
    session: Option<String>,
    all: bool,
    older_than: Option<u64>,
    keep: Option<usize>,
) -> Result<()> {
    if all {
        let count = TaskStore::clean_all().await.context("failed to clean all sessions")?;
        println!("Deleted {count} session(s).");
    } else if let Some(days) = older_than {
        let count = TaskStore::clean_older_than(days)
            .await
            .context("failed to clean old sessions")?;
        if count > 0 {
            println!("Deleted {count} session(s) older than {days} days.");
        } else {
            println!("No old sessions to clean.");
        }
    } else if let Some(n) = keep {
        let count = TaskStore::keep_recent(n)
            .await
            .context("failed to prune sessions")?;
        if count > 0 {
            println!("Deleted {count} session(s), kept {n} most recent.");
        } else {
            println!("Nothing to clean.");
        }
    } else if let Some(id) = session {
        TaskStore::clean_session(&id)
            .await
            .context("failed to delete session")?;
        println!("Session {id} deleted.");
    } else {
        anyhow::bail!("No action specified. Usage: conductor clean --session <id> | --all | --older-than <days> | --keep <n>");
    }
    Ok(())
}

async fn run_interactive(
    project_path: String,
    musician_count: usize,
    conductor_model: String,
    musician_model: String,
    max_turns: u32,
) -> Result<()> {
    // Validate Claude CLI and models before starting
    validate_claude_cli()
        .await
        .context("Claude CLI validation failed")?;
    validate_model(&conductor_model)
        .context(format!("invalid conductor model: {conductor_model}"))?;
    validate_model(&musician_model)
        .context(format!("invalid musician model: {musician_model}"))?;

    let session_id = &Uuid::new_v4().to_string()[..8];

    let config = OrchestraConfig {
        project_path,
        task_description: String::new(), // interactive mode — task comes from TUI
        musician_count,
        conductor_model,
        musician_model,
        max_turns,
        dry_run: false,
        session_id: session_id.to_string(),
        reference_session_id: None,
        verification: None,
        headless: false,
    };

    info!(session_id = %session_id, "Starting interactive session");

    let (mut orchestra, state_rx, action_tx) = Orchestra::new(config);

    // TuiApp is Send (holds watch::Receiver + mpsc::Sender), so it can be spawned.
    // Orchestra is !Send, so it must run inline on the current task.
    let mut tui = conductor_tui::app::TuiApp::new(state_rx, action_tx);
    let tui_handle = tokio::spawn(async move { tui.run().await });
    let abort_handle = tui_handle.abort_handle();

    let tui_error = tokio::select! {
        result = async {
            orchestra.run().await?;
            orchestra.event_loop().await
        } => {
            result.map_err(|e| anyhow::anyhow!("{e}"))?;
            None
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down");
            None
        }
        tui_result = tui_handle => {
            // TUI exited (user pressed q or error)
            match tui_result {
                Ok(Err(e)) => Some(e),
                _ => None,
            }
        }
    };

    // Ensure orchestra cleans up musician processes regardless of which branch fired
    orchestra.shutdown().await;
    abort_handle.abort();

    if let Some(e) = tui_error {
        return Err(e);
    }
    Ok(())
}

async fn run_headless(
    project_path: String,
    task_description: String,
    musician_count: usize,
    conductor_model: String,
    musician_model: String,
    max_turns: u32,
    dry_run: bool,
    reference_session_id: Option<String>,
) -> Result<()> {
    validate_claude_cli()
        .await
        .context("Claude CLI validation failed")?;
    validate_model(&conductor_model)
        .context(format!("invalid conductor model: {conductor_model}"))?;
    validate_model(&musician_model)
        .context(format!("invalid musician model: {musician_model}"))?;

    let session_id = &Uuid::new_v4().to_string()[..8];

    let config = OrchestraConfig {
        project_path,
        task_description,
        musician_count,
        conductor_model,
        musician_model,
        max_turns,
        dry_run,
        session_id: session_id.to_string(),
        reference_session_id,
        verification: None,
        headless: true,
    };

    info!(session_id = %session_id, "Starting headless session");

    let (mut orchestra, _state_rx, _action_tx) = Orchestra::new(config);

    tokio::select! {
        result = async {
            orchestra.run().await?;
            orchestra.event_loop().await
        } => {
            if let Err(ref e) = result {
                print_json_parse_output(e);
            }
            result.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down");
        }
    }

    Ok(())
}

async fn resume_session(session_id: String) -> Result<()> {
    let resolved = TaskStore::resolve_id(&session_id)
        .await
        .with_context(|| format!("session '{session_id}' not found"))?;

    let store = TaskStore::new(&resolved);
    let session = store
        .load_session()
        .await
        .context("failed to load session")?
        .with_context(|| format!("session '{resolved}' not found"))?;

    validate_claude_cli()
        .await
        .context("Claude CLI validation failed")?;

    let config = session.config;
    info!(session_id = %resolved, "Resuming session");

    let (mut orchestra, _state_rx, _action_tx) = Orchestra::new(config);

    tokio::select! {
        result = async {
            orchestra.run().await?;
            orchestra.event_loop().await
        } => {
            if let Err(ref e) = result {
                print_json_parse_output(e);
            }
            result.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down");
        }
    }

    Ok(())
}

/// Print raw Claude output when a JsonParse error occurs, so users see what Claude said.
fn print_json_parse_output(e: &CoreError) {
    if let CoreError::JsonParse { raw_output, .. } = e {
        if !raw_output.is_empty() {
            eprintln!("\n--- Claude's response ---\n{raw_output}\n--- end response ---");
        }
    }
}
