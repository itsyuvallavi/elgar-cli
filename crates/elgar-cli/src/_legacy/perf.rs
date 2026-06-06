//! Legacy local performance baseline code.
//!
//! This is archived from the old CLI surface. It is not part of the active
//! raw-chat CLI path.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use elgar_core::{
    controller::Controller,
    event::ProviderOutput,
    provider::{ControllerProvider, ProviderError, ProviderRequestMetadata},
    session::Session,
};

const TUI_ITERATIONS: usize = 100;
const PROVIDER_ITERATIONS: usize = 100;
const TUI_TRANSCRIPT_LINES: [usize; 4] = [1, 10, 50, 100];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfBaselineReport {
    pub tui: Vec<TuiRenderBaseline>,
    pub provider: ProviderPhaseBaseline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiRenderBaseline {
    pub transcript_lines: usize,
    pub iterations: usize,
    pub rendered_bytes: usize,
    pub p50_micros: u128,
    pub max_micros: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPhaseBaseline {
    pub iterations: usize,
    pub request_start_to_first_read_p50_micros: u128,
    pub request_start_to_completion_p50_micros: u128,
    pub controller_turn_p50_micros: u128,
    pub controller_turn_max_micros: u128,
}

pub fn run_perf_baseline() -> PerfBaselineReport {
    PerfBaselineReport {
        tui: measure_tui_render_baselines(),
        provider: measure_provider_phase_baseline(),
    }
}

pub fn render_perf_baseline(report: &PerfBaselineReport) -> String {
    let mut output = vec![
        "Elgar local performance baseline".to_string(),
        "mode: no-network".to_string(),
        "live_provider: not measured by this command".to_string(),
        String::new(),
        "TUI render/update".to_string(),
    ];

    for baseline in &report.tui {
        output.push(format!(
            "  transcript_lines={} iterations={} rendered_bytes={} p50_us={} max_us={}",
            baseline.transcript_lines,
            baseline.iterations,
            baseline.rendered_bytes,
            baseline.p50_micros,
            baseline.max_micros
        ));
    }

    output.extend([
        String::new(),
        "Provider phases (stub, no-network)".to_string(),
        format!(
            "  iterations={} request_start_to_first_read_p50_us={} request_start_to_completion_p50_us={}",
            report.provider.iterations,
            report.provider.request_start_to_first_read_p50_micros,
            report.provider.request_start_to_completion_p50_micros
        ),
        format!(
            "  controller_turn_p50_us={} controller_turn_max_us={}",
            report.provider.controller_turn_p50_micros, report.provider.controller_turn_max_micros
        ),
    ]);

    output.join("\n")
}

pub fn render_latest_trace_perf_summary(project_root: &Path) -> Result<String, String> {
    let (trace_path, summary) = latest_trace_perf_summary(project_root)?;
    Ok(render_trace_perf_summary(&trace_path, &summary))
}

fn latest_trace_perf_summary(project_root: &Path) -> Result<(PathBuf, serde_json::Value), String> {
    let trace_dir = project_root.join(".elgar/traces");
    let mut traces = fs::read_dir(&trace_dir)
        .map_err(|error| {
            format!(
                "could not read trace directory {}: {error}",
                trace_dir.display()
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .map(|path| {
            let modified = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, path)
        })
        .collect::<Vec<_>>();
    traces.sort_by(|left, right| right.0.cmp(&left.0));

    for (_modified, path) in traces {
        if let Some(summary) = latest_summary_in_trace(&path)? {
            return Ok((path, summary));
        }
    }

    Err(format!(
        "no turn_perf_summary entries found under {}",
        trace_dir.display()
    ))
}

fn latest_summary_in_trace(path: &Path) -> Result<Option<serde_json::Value>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut latest = None;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let value = serde_json::from_str::<serde_json::Value>(line)
            .map_err(|error| format!("invalid trace json in {}: {error}", path.display()))?;
        if value.get("kind").and_then(serde_json::Value::as_str) == Some("turn_perf_summary") {
            latest = value.get("metadata").cloned();
        }
    }
    Ok(latest)
}

fn render_trace_perf_summary(trace_path: &Path, summary: &serde_json::Value) -> String {
    let mut output = vec![
        "Elgar latest turn performance".to_string(),
        format!("trace: {}", trace_path.display()),
        format!("route: {}", string_field(summary, "route")),
        format!(
            "provider_requests: {} · actions: {} · tools_exposed: {} · tool_calls: {}",
            number_field(summary, "provider_request_count"),
            number_field(summary, "action_count"),
            number_field(summary, "total_tool_count"),
            number_field(summary, "tool_call_count")
        ),
        format!(
            "provider_time_ms: {} · first_chunk_ms: {}",
            optional_number_field(summary, "total_provider_duration_millis"),
            optional_number_field(summary, "first_chunk_latency_millis")
        ),
        format!(
            "tokens: prompt {} · completion {} · total {}",
            optional_number_field(summary, "prompt_tokens"),
            optional_number_field(summary, "completion_tokens"),
            optional_number_field(summary, "total_tokens")
        ),
        format!(
            "context_shape: messages {} · request_bytes {}",
            number_field(summary, "message_count"),
            number_field(summary, "serialized_request_bytes")
        ),
        format!(
            "output_shape: visible_chars {} · thinking_chars {}",
            number_field(summary, "visible_text_chars"),
            number_field(summary, "thinking_chars")
        ),
    ];

    if let Some(requests) = summary
        .get("provider_requests")
        .and_then(serde_json::Value::as_array)
        .filter(|requests| !requests.is_empty())
    {
        output.push(String::new());
        output.push("Provider requests".to_string());
        for request in requests {
            output.push(format!(
                "  - {} · mode {} · backend {} · tools {} · messages {} · bytes {} · duration_ms {} · ttft_ms {} · tok/s {} · tokens {} · reasoning_tokens {}",
                string_field(request, "request_id"),
                string_field(request, "request_mode"),
                string_field(request, "backend"),
                number_field(request, "tool_count"),
                number_field(request, "message_count"),
                number_field(request, "serialized_request_bytes"),
                optional_number_field(request, "total_duration_millis"),
                optional_number_field(request, "provider_time_to_first_token_millis"),
                optional_milli_number_field(request, "provider_tokens_per_second_milli"),
                optional_number_field(request, "total_tokens"),
                optional_number_field(request, "reasoning_output_tokens")
            ));
        }
    }

    output.join("\n")
}

fn string_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("n/a")
        .to_string()
}

fn number_field(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn optional_number_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|number| number.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn optional_milli_number_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|number| format!("{:.1}", number as f64 / 1000.0))
        .unwrap_or_else(|| "n/a".to_string())
}

fn measure_tui_render_baselines() -> Vec<TuiRenderBaseline> {
    TUI_TRANSCRIPT_LINES
        .into_iter()
        .map(measure_tui_render_baseline)
        .collect()
}

fn measure_tui_render_baseline(transcript_lines: usize) -> TuiRenderBaseline {
    let mut shell = elgar_tui::TuiShell::new();
    for index in 0..transcript_lines {
        shell.push_local_message(format!("local transcript line {index}"));
    }

    let rendered_bytes = shell.render().len();
    let mut samples = Vec::with_capacity(TUI_ITERATIONS);
    for _ in 0..TUI_ITERATIONS {
        let mut candidate = shell.clone();
        let started = Instant::now();
        candidate.push_local_message("local update");
        let _rendered = candidate.render();
        samples.push(started.elapsed());
    }

    TuiRenderBaseline {
        transcript_lines,
        iterations: TUI_ITERATIONS,
        rendered_bytes,
        p50_micros: p50_micros(samples.clone()),
        max_micros: max_micros(&samples),
    }
}

fn measure_provider_phase_baseline() -> ProviderPhaseBaseline {
    let mut first_read_samples = Vec::with_capacity(PROVIDER_ITERATIONS);
    let mut completion_samples = Vec::with_capacity(PROVIDER_ITERATIONS);
    let mut turn_samples = Vec::with_capacity(PROVIDER_ITERATIONS);

    for _ in 0..PROVIDER_ITERATIONS {
        let provider = TimedStubProvider::default();
        let timings = Arc::clone(&provider.timings);
        let controller = Controller::new(provider);
        let mut session = Session::new("perf-baseline-session", ".", ".");
        let started = Instant::now();

        controller.turn(&mut session, "what does the harness do?");
        turn_samples.push(started.elapsed());

        let timings = timings.lock().expect("timings poisoned");
        if let Some(first_read) = timings.first_read {
            first_read_samples.push(first_read.duration_since(timings.request_start));
        }
        if let Some(completion) = timings.completion {
            completion_samples.push(completion.duration_since(timings.request_start));
        }
    }

    ProviderPhaseBaseline {
        iterations: PROVIDER_ITERATIONS,
        request_start_to_first_read_p50_micros: p50_micros(first_read_samples),
        request_start_to_completion_p50_micros: p50_micros(completion_samples),
        controller_turn_p50_micros: p50_micros(turn_samples.clone()),
        controller_turn_max_micros: max_micros(&turn_samples),
    }
}

fn p50_micros(mut samples: Vec<Duration>) -> u128 {
    if samples.is_empty() {
        return 0;
    }

    samples.sort_unstable();
    samples[samples.len() / 2].as_micros()
}

fn max_micros(samples: &[Duration]) -> u128 {
    samples
        .iter()
        .max()
        .copied()
        .unwrap_or_default()
        .as_micros()
}

#[derive(Debug, Clone)]
struct TimedStubProvider {
    timings: Arc<Mutex<TimedProviderPhases>>,
}

impl Default for TimedStubProvider {
    fn default() -> Self {
        Self {
            timings: Arc::new(Mutex::new(TimedProviderPhases {
                request_start: Instant::now(),
                first_read: None,
                completion: None,
            })),
        }
    }
}

impl ControllerProvider for TimedStubProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        let mut timings = self.timings.lock().expect("timings poisoned");
        timings.request_start = Instant::now();
        timings.first_read = None;
        timings.completion = None;
        ProviderRequestMetadata::new("timed-stub-provider", None, "timed-stub-request-1")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        let mut timings = self.timings.lock().expect("timings poisoned");
        timings.first_read = Some(Instant::now());
        let output = ProviderOutput::new("timed stub provider response");
        timings.completion = Some(Instant::now());
        Ok(output)
    }
}

#[derive(Debug, Clone)]
struct TimedProviderPhases {
    request_start: Instant,
    first_read: Option<Instant>,
    completion: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::{
        render_latest_trace_perf_summary, render_perf_baseline, PerfBaselineReport,
        ProviderPhaseBaseline, TuiRenderBaseline,
    };

    #[test]
    fn renders_no_network_performance_baseline_report() {
        let report = PerfBaselineReport {
            tui: vec![TuiRenderBaseline {
                transcript_lines: 10,
                iterations: 100,
                rendered_bytes: 512,
                p50_micros: 12,
                max_micros: 30,
            }],
            provider: ProviderPhaseBaseline {
                iterations: 100,
                request_start_to_first_read_p50_micros: 1,
                request_start_to_completion_p50_micros: 2,
                controller_turn_p50_micros: 3,
                controller_turn_max_micros: 5,
            },
        };

        let rendered = render_perf_baseline(&report);

        assert!(rendered.contains("mode: no-network"));
        assert!(rendered.contains("TUI render/update"));
        assert!(rendered.contains("Provider phases (stub, no-network)"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn renders_latest_trace_perf_summary_report() {
        let root =
            std::env::temp_dir().join(format!("elgar-perf-trace-report-{}", std::process::id()));
        let trace_dir = root.join(".elgar/traces");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&trace_dir).unwrap();
        std::fs::write(
            trace_dir.join("session.jsonl"),
            r#"{"kind":"turn_perf_summary","metadata":{"route":"chat","provider_request_count":1,"request_modes":["plain_chat"],"total_tool_count":0,"action_count":0,"total_provider_duration_millis":12889,"message_count":3,"serialized_request_bytes":1094,"prompt_tokens":233,"completion_tokens":568,"total_tokens":801,"visible_text_chars":63,"thinking_chars":2149,"tool_call_count":0,"provider_requests":[{"request_id":"request-1","provider":"lm-studio","model":"qwen","request_mode":"plain_chat","backend":"lm_studio_native_chat","tool_count":0,"stream":false,"message_count":3,"serialized_request_bytes":1094,"total_duration_millis":12889,"provider_time_to_first_token_millis":420,"provider_tokens_per_second_milli":54050,"reasoning_output_tokens":128,"prompt_tokens":233,"completion_tokens":568,"total_tokens":801,"visible_text_chars":63,"thinking_chars":2149,"tool_call_count":0}]}}"#,
        )
        .unwrap();

        let rendered = render_latest_trace_perf_summary(&root).unwrap();

        assert!(rendered.contains("Elgar latest turn performance"));
        assert!(rendered.contains("route: chat"));
        assert!(rendered.contains("provider_requests: 1"));
        assert!(rendered.contains("provider_time_ms: 12889"));
        assert!(rendered.contains("tokens: prompt 233"));
        assert!(rendered.contains("thinking_chars 2149"));
        assert!(rendered.contains("mode plain_chat"));
        assert!(rendered.contains("backend lm_studio_native_chat"));
        assert!(rendered.contains("ttft_ms 420"));
        assert!(rendered.contains("tok/s 54.0"));
        assert!(rendered.contains("reasoning_tokens 128"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn latest_trace_perf_summary_reports_newest_error_trace() {
        let root = std::env::temp_dir().join(format!(
            "elgar-perf-trace-error-report-{}",
            std::process::id()
        ));
        let trace_dir = root.join(".elgar/traces");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&trace_dir).unwrap();
        let old_trace = trace_dir.join("old.jsonl");
        let new_trace = trace_dir.join("new.jsonl");
        std::fs::write(
            &old_trace,
            r#"{"kind":"turn_perf_summary","metadata":{"route":"execute","provider_request_count":1,"total_provider_duration_millis":999,"total_tool_count":1,"action_count":1}}"#,
        )
        .unwrap();
        std::fs::write(
            &new_trace,
            r#"{"kind":"turn_perf_summary","metadata":{"provider_request_count":1,"request_modes":["plain_chat"],"total_tool_count":0,"action_count":0,"provider_requests":[{"request_id":"request-error","provider":"lm-studio","model":"qwen","request_mode":"plain_chat","tool_count":0}]}}"#,
        )
        .unwrap();

        let rendered = render_latest_trace_perf_summary(&root).unwrap();

        assert!(rendered.contains("trace: "));
        assert!(rendered.contains("new.jsonl"));
        assert!(rendered.contains("provider_time_ms: n/a"));
        assert!(rendered.contains("request-error"));

        let _ = std::fs::remove_dir_all(root);
    }
}
