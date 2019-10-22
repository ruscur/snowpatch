//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2019 IBM Corporation
// Authors:
//     Russell Currey <ruscur@russell.cc>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// email.rs - snowpatch email sending functionality
//
#![allow(warnings)]
use lettre::smtp::authentication::{Credentials, Mechanism};
use lettre::smtp::ConnectionReuseParameters;
use lettre::{SmtpClient, SmtpTransport, Transport};
use lettre_email::{EmailBuilder, Mailbox};
use patchwork::{Patch, TestResult, TestState};
use settings;
use settings::Config;
use std::error::Error;

struct EmailReport {
    patch: Patch,
    errors: Vec<TestResult>,
    warnings: Vec<TestResult>,
    successes: Vec<TestResult>,
    apply_patch: TestResult,
    settings: settings::Email,
}

impl EmailReport {
    pub fn new(patch: Patch, results: Vec<TestResult>, settings: settings::Email) -> EmailReport {
        let mut errors: Vec<TestResult> = Vec::new();
        let mut warnings: Vec<TestResult> = Vec::new();
        let mut successes: Vec<TestResult> = Vec::new();
        // we create this pointless result because we know we're always going to have an
        //"apply_patch" test, but the compiler doesn't.  this is a bit hacky.
        let mut apply_patch = TestResult {
            state: TestState::Pending,
            ..Default::default()
        };

        for result in results {
            if result.context.as_ref().unwrap() == "apply_patch" {
                apply_patch = result;
                // we don't want to treat it as a normal test
                continue;
            }
            match result.state {
                TestState::Fail => errors.push(result),
                TestState::Warning => warnings.push(result),
                TestState::Success => successes.push(result),
                _ => (),
            }
        }

        EmailReport {
            patch: patch,
            errors: errors,
            warnings: warnings,
            successes: successes,
            apply_patch: apply_patch,
            settings: settings,
        }
    }
    pub fn populate_to(&self, builder: EmailBuilder) -> EmailBuilder {
        let builder = match &self.patch.submitter.name {
            Some(name) => builder.to((&self.patch.submitter.email, name)),
            None => builder.to(self.patch.submitter.email.as_str()),
        };
        builder.to(self.patch.project.list_email.as_str())
    }

    fn format_apply(&self) -> String {
        match &self.apply_patch.state {
            TestState::Success => format!(
                "Your patch was {}\n",
                &self
                    .apply_patch
                    .description
                    .as_ref()
                    .unwrap()
                    .to_lowercase()
            ),
            TestState::Fail => format!("Your patch failed to apply to any branch.\n"),
            _ => String::from("We've somehow lost track of where we applied your patch...\n"),
        }
    }

    fn format_error(&self, result: TestResult) -> String {
        let mut report = String::new();

        report.push_str(
            format!(
                "The test {} reported the following: {}\n \
                 You can see more details here: {}\n",
                result.context.unwrap(),
                result.description.unwrap(),
                result.target_url.unwrap()
            )
            .as_str(),
        );

        report
    }

    /// Produce a neat report string for a given individual test result
    /// TODO: we assume all TestResult fields are populated.
    /// if produced by snowpatch, they should be, but still.
    pub fn populate_body(&self, builder: EmailBuilder) -> EmailBuilder {
        let mut body = String::new();
        body.push_str("Thanks for your contribution, unfortunately we've found some issues.\n\n");
        body.push_str(&self.format_apply());
        body.push_str("\n");
        for error in &self.errors {
            body.push_str(&self.format_error(error.clone()));
            body.push_str("\n");
        }
        builder
    }
}
/// Only supports localhost:25 unauthenticated currently.
/// This should most likely be configured as a relay to a remote service.
fn get_mailer() -> SmtpTransport {
    SmtpClient::new_unencrypted_localhost().unwrap().transport()
}

/// Mail the author of a series the results
pub fn send_series_results(
    patch: &Patch,
    results: Vec<TestResult>,
    settings: &settings::Email,
) -> Result<(), Box<dyn Error>> {
    let mut mailer = get_mailer();
    let mut builder = EmailBuilder::new();
    let report = EmailReport::new(patch.clone(), results, settings.clone());
    builder = report.populate_to(builder);
    builder = report.populate_body(builder);
    match mailer.send(builder.build()?.into()) {
        Ok(resp) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod test {
    use lettre::{
        smtp::authentication::Credentials, smtp::authentication::Mechanism,
        smtp::ConnectionReuseParameters, SmtpClient, SmtpTransport, Transport,
    };
    use lettre_email::{mime::TEXT_PLAIN, Email};
    use std::path::Path;

    #[test]
    #[ignore] // trigger with "cargo test --ignored" if you have a SMTP server
    fn send_email() -> Result<(), String> {
        let test_email: Email = Email::builder()
            .to(("bob@mailinator.com", "Big Bobby Boy"))
            .from("fake@notreal.com")
            .subject("snowpatch SMTP test")
            .text("Hello from snowpatch!")
            .build()
            .unwrap();
        let mut mailer: SmtpTransport =
            SmtpClient::new_unencrypted_localhost().unwrap().transport();
        let result = match mailer.send(test_email.into()) {
            Ok(res) => {
                println!(
                    "Success, got code {} and message {}",
                    res.code,
                    res.first_line().unwrap()
                );
                Ok(())
            }
            Err(e) => Err(format!("Failed to send email: {}", e)),
        };
        mailer.close();
        result
    }
}
