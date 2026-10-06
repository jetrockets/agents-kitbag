//! Fakes for the two doors to the outside: commands and requests.

use std::sync::Mutex;

use crate::exec::{CommandRunner, Output};
use crate::http::{Http, Request, Response};

type Answer = Box<dyn Fn(&str, &[&str], Option<&str>) -> Option<Output> + Send + Sync>;

/// Answers commands from rules and remembers every call.
#[derive(Default)]
pub struct FakeRunner {
    rules: Vec<Answer>,
    pub calls: Mutex<Vec<Call>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub program: String,
    pub args: Vec<String>,
    pub input: Option<String>,
}

pub fn ok(stdout: &str) -> Output {
    Output {
        code: 0,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

pub fn fail(code: i32, stderr: &str) -> Output {
    Output {
        code,
        stdout: String::new(),
        stderr: stderr.to_owned(),
    }
}

impl FakeRunner {
    /// Answers any call whose program ends with `program` and whose
    /// arguments contain `arg`.
    pub fn on(mut self, program: &'static str, arg: &'static str, output: Output) -> Self {
        self.rules.push(Box::new(move |p, args, input| {
            // A command can also arrive on stdin (`security -i`).
            let named = args.iter().any(|a| a.contains(arg))
                || input.is_some_and(|input| input.starts_with(arg));
            (p.ends_with(program) && named).then(|| output.clone())
        }));
        self
    }

    /// Answers with whatever `rule` does, for answers that change as the
    /// calls go by.
    pub fn when(
        mut self,
        rule: impl Fn(&str, &[&str], Option<&str>) -> Option<Output> + Send + Sync + 'static,
    ) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    /// Everything that was on a command line, to prove a secret was not.
    pub fn all_arguments(&self) -> String {
        self.calls()
            .iter()
            .map(|c| format!("{} {}", c.program, c.args.join(" ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Output {
        self.calls.lock().unwrap().push(Call {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            input: input.map(str::to_owned),
        });
        self.rules
            .iter()
            .find_map(|rule| rule(program, args, input))
            .unwrap_or_else(|| fail(-1, "no such command"))
    }
}

/// Answers requests by the first rule whose text is in the URL.
#[derive(Default)]
pub struct FakeHttp {
    rules: Vec<(&'static str, Result<Response, String>)>,
    pub requests: Mutex<Vec<Request>>,
}

impl FakeHttp {
    pub fn on(mut self, url_part: &'static str, status: u16, body: &str) -> Self {
        self.rules.push((
            url_part,
            Ok(Response {
                status,
                body: body.to_owned(),
            }),
        ));
        self
    }

    pub fn down(mut self, url_part: &'static str) -> Self {
        self.rules.push((url_part, Err("network error".to_owned())));
        self
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Http for FakeHttp {
    fn send(&self, request: &Request) -> Result<Response, String> {
        self.requests.lock().unwrap().push(request.clone());
        self.rules
            .iter()
            .find(|(part, _)| request.url.contains(part))
            .map(|(_, answer)| answer.clone())
            .unwrap_or_else(|| Err(format!("unexpected request to {}", request.url)))
    }
}
