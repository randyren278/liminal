use std::path::Path;

use crate::ledger_view::CalibrationReportView;

pub fn format_report(path: &Path, report: &CalibrationReportView) -> String {
    format!(
        "CALIBRATE / EXPLICIT LABELS\n\nlabels           {}\nmatched          {}\nunmatched        {}\naccuracy         {:.3}\nBrier score       {:.3}\nprecision         {:.3}\nrecall            {:.3}\n\nSource: {}\nThis is an offline score against human/reference labels. It does not retune the live heuristic.",
        report.labels_total,
        report.matched_labels,
        report.unmatched_labels,
        report.accuracy,
        report.brier_score,
        report.positive_precision,
        report.positive_recall,
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_keeps_explicit_label_provenance_and_metrics_visible() {
        let report = CalibrationReportView {
            labels_total: 4,
            matched_labels: 3,
            unmatched_labels: 1,
            accuracy: 0.667,
            brier_score: 0.125,
            positive_precision: 0.5,
            positive_recall: 1.0,
        };
        let rendered = format_report(Path::new("trial-labels.jsonl"), &report);
        assert!(rendered.contains("CALIBRATE / EXPLICIT LABELS"));
        assert!(rendered.contains("matched          3"));
        assert!(rendered.contains("Brier score       0.125"));
        assert!(rendered.contains("trial-labels.jsonl"));
        assert!(rendered.contains("does not retune the live heuristic"));
    }
}
