//! The machine readable output of `postino run`: JSON lines and JUnit XML.

use std::time::Duration;

use postino_core::TestResult;
use serde_json::json;

use crate::exit;

/// One request of a run, as the reports need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The workspace relative id, `auth/login.postino`.
    pub id: String,
    /// The method, empty when the file could not be read.
    pub method: String,
    /// The URL sent, without its query string, when the request was built.
    pub url: Option<String>,
    /// The response status, when one came back.
    pub status: Option<u16>,
    /// How long it took.
    pub time: Duration,
    /// The response body size, when one came back.
    pub size: Option<usize>,
    /// Every `test()`, in order.
    pub tests: Vec<TestResult>,
    /// Why it did not run to the end.
    pub error: Option<String>,
    /// The unresolved `{{ }}` markers and the certificate warning.
    pub warnings: Vec<String>,
    /// Its exit code: [`exit::SUCCESS`], [`exit::FAILED`], [`exit::NETWORK`] or [`exit::FILE`].
    pub code: u8,
}

impl Record {
    /// Whether it ran to the end with every test green.
    pub fn passed(&self) -> bool {
        self.code == exit::SUCCESS
    }
}

/// The totals of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Requests run.
    pub requests: usize,
    /// Requests that did not pass.
    pub failed: usize,
    /// `test()` calls.
    pub tests: usize,
    /// `test()` calls that passed.
    pub passed_tests: usize,
    /// The sum of the request times.
    pub time: Duration,
    /// The exit code of the run.
    pub code: u8,
}

impl Totals {
    /// The totals of `records`.
    pub fn of(records: &[Record]) -> Self {
        records
            .iter()
            .fold(Totals::default(), |totals, record| Totals {
                requests: totals.requests + 1,
                failed: totals.failed + usize::from(!record.passed()),
                tests: totals.tests + record.tests.len(),
                passed_tests: totals.passed_tests
                    + record.tests.iter().filter(|t| t.passed).count(),
                time: totals.time + record.time,
                code: exit::worst(totals.code, record.code),
            })
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The JSON line of one request.
pub fn json_line(record: &Record) -> String {
    let tests: Vec<_> = record
        .tests
        .iter()
        .map(|test| json!({"name": test.name, "passed": test.passed, "message": test.message}))
        .collect();
    json!({
        "type": "request",
        "request": record.id,
        "method": record.method,
        "url": record.url,
        "status": record.status,
        "time_ms": millis(record.time),
        "size": record.size,
        "passed": record.passed(),
        "tests": tests,
        "error": record.error,
        "warnings": record.warnings,
    })
    .to_string()
}

/// The last JSON line, with the totals.
pub fn json_summary(totals: &Totals) -> String {
    json!({
        "type": "summary",
        "requests": totals.requests,
        "failed": totals.failed,
        "tests": totals.tests,
        "passed_tests": totals.passed_tests,
        "time_ms": millis(totals.time),
        "exit_code": totals.code,
    })
    .to_string()
}

/// Every line of the JSON output, each ending in a newline.
pub fn json(records: &[Record]) -> String {
    let mut out = String::new();
    for record in records {
        out.push_str(&json_line(record));
        out.push('\n');
    }
    out.push_str(&json_summary(&Totals::of(records)));
    out.push('\n');
    out
}

/// Text escaped for an XML attribute or element.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            // Control characters are not allowed in XML 1.0.
            c if c.is_control() && c != '\t' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// The test cases of a request in the JUnit report: its tests, plus one for an error or for a
/// request without tests.
fn case_count(record: &Record) -> usize {
    record.tests.len() + usize::from(record.error.is_some() || record.tests.is_empty())
}

fn seconds(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64())
}

/// The JUnit XML report: one test suite per request, one test case per `test()`. A request that
/// did not run to the end adds an `error` case, and one without tests a case named `request`.
pub fn junit(records: &[Record]) -> String {
    let totals = Totals::of(records);
    let errors = records.iter().filter(|r| r.error.is_some()).count();
    let failures: usize = records
        .iter()
        .map(|r| r.tests.iter().filter(|t| !t.passed).count())
        .sum();
    let total_cases: usize = records.iter().map(case_count).sum();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuites name=\"postino run\" tests=\"{total_cases}\" failures=\"{failures}\" errors=\"{errors}\" time=\"{}\">\n",
        seconds(totals.time)
    ));
    for record in records {
        let id = xml_escape(record.id.strip_suffix(".postino").unwrap_or(&record.id));
        let red = record.tests.iter().filter(|t| !t.passed).count();
        let error = usize::from(record.error.is_some());
        let tests = case_count(record);
        out.push_str(&format!(
            "  <testsuite name=\"{id}\" tests=\"{tests}\" failures=\"{red}\" errors=\"{error}\" time=\"{}\">\n",
            seconds(record.time)
        ));
        for test in &record.tests {
            let name = xml_escape(&test.name);
            if test.passed {
                out.push_str(&format!(
                    "    <testcase classname=\"{id}\" name=\"{name}\"/>\n"
                ));
            } else {
                let message = xml_escape(test.message.as_deref().unwrap_or("failed"));
                out.push_str(&format!(
                    "    <testcase classname=\"{id}\" name=\"{name}\">\n      <failure message=\"{message}\"/>\n    </testcase>\n"
                ));
            }
        }
        match &record.error {
            Some(message) => out.push_str(&format!(
                "    <testcase classname=\"{id}\" name=\"request\" time=\"{}\">\n      <error message=\"{}\"/>\n    </testcase>\n",
                seconds(record.time),
                xml_escape(message)
            )),
            None if record.tests.is_empty() => out.push_str(&format!(
                "    <testcase classname=\"{id}\" name=\"request\" time=\"{}\"/>\n",
                seconds(record.time)
            )),
            None => {}
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn test(name: &str, passed: bool) -> TestResult {
        TestResult {
            name: name.to_string(),
            passed,
            message: (!passed).then(|| "expected 401 to be 200".to_string()),
        }
    }

    fn records() -> Vec<Record> {
        vec![
            Record {
                id: "auth/login.postino".into(),
                method: "POST".into(),
                url: Some("http://api/login".into()),
                status: Some(200),
                time: Duration::from_millis(84),
                size: Some(12),
                tests: vec![test("status is 200", true)],
                error: None,
                warnings: Vec::new(),
                code: exit::SUCCESS,
            },
            Record {
                id: "auth/me.postino".into(),
                method: "GET".into(),
                url: Some("http://api/me".into()),
                status: Some(401),
                time: Duration::from_millis(12),
                size: Some(0),
                tests: vec![test("status is 200", false)],
                error: None,
                warnings: Vec::new(),
                code: exit::FAILED,
            },
            Record {
                id: "down.postino".into(),
                method: "GET".into(),
                url: Some("http://down/".into()),
                status: None,
                time: Duration::from_millis(3),
                size: None,
                tests: Vec::new(),
                error: Some("sending the request failed: <refused>".into()),
                warnings: Vec::new(),
                code: exit::NETWORK,
            },
        ]
    }

    #[test]
    fn totals_add_up_and_keep_the_worst_code() {
        let totals = Totals::of(&records());
        assert_eq!(totals.requests, 3);
        assert_eq!(totals.failed, 2);
        assert_eq!(totals.tests, 2);
        assert_eq!(totals.passed_tests, 1);
        assert_eq!(totals.time, Duration::from_millis(99));
        assert_eq!(totals.code, exit::NETWORK);
    }

    #[test]
    fn json_has_a_line_per_request_and_a_summary() {
        let out = json(&records());
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["request"], "auth/login.postino");
        assert_eq!(lines[0]["passed"], true);
        assert_eq!(lines[1]["tests"][0]["message"], "expected 401 to be 200");
        assert_eq!(lines[2]["status"], serde_json::Value::Null);
        assert_eq!(lines[3]["type"], "summary");
        assert_eq!(lines[3]["exit_code"], 3);
    }

    #[test]
    fn junit_has_a_suite_per_request_and_escapes_text() {
        let xml = junit(&records());
        assert!(xml.starts_with("<?xml"));
        assert!(
            xml.contains(
                "<testsuites name=\"postino run\" tests=\"3\" failures=\"1\" errors=\"1\""
            )
        );
        assert!(xml.contains("<testsuite name=\"auth/login\" tests=\"1\" failures=\"0\""));
        assert!(xml.contains("<failure message=\"expected 401 to be 200\"/>"));
        assert!(xml.contains("<error message=\"sending the request failed: &lt;refused&gt;\"/>"));
        assert!(xml.ends_with("</testsuites>\n"));
    }
}
