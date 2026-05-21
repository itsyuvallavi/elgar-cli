use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
        render_perf_baseline, PerfBaselineReport, ProviderPhaseBaseline, TuiRenderBaseline,
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
}
