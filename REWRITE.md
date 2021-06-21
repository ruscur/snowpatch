### snowpatch rewrite goals

Doesn't have to be a complete rewrite, but snowpatch was written a long time ago with different goals, the Rust library ecosystem has thoroughly changed and the actual things we want snowpatch to do have also changed.  As a result, this branch is an experiment in a clean rewrite to see what we really need.

Primary goals include:

- a threading model at its core and not bolted on
- a reworked argument and configuration file
- proper abstraction for test runners
- better, cleaner usage of third-party libraries
- replace toml config file with something else more writable

### modularity

split out what snowpatch does into the following:

- patchwork scanner that does git things
- test watcher that gets metadata from a git branch
  - watches github actions / gitlab CI
  - keeps patchwork in sync
- emailer that figures out test completion on patchwork and sends

### TODO

Next things on the list at present:

- flesh out what we want in the config file
- write tests for config file parsing and validating
- DONE investigate HTTP library options
  - happy with ureq unless we end up actually wanting async
- watchcat (Patchwork scanner)
  - basic threading scans and assesses state.
  -

### Notes
#### ureq

Gonna try ureq and see if it's enough so we don't have to reqwest (and thus hyper)

#### anyhow

Since snowpatch is an application and not a library, but we consume a lot of different libraries, error handling is a pain.  Everything returns different error types that we don't care about, we just want to know what went wrong.  Using anyhow instead of being forced to box errors is nicer.

anyhow allows you to add context to anything that doesn't already have it, providing a middle ground to detailed error handling.

#### documentation

Given so much of snowpatch's configuration was (and will continue to be) translated from config file straight into a struct, properly documenting the struct translates directly to workable documentation.

#### validation

The entire config file should be both parsed and validated for as much as we can check for before the data can potentially be used somewhere.

#### timeouts

Job runners should have sensible timeouts, probably defined in config.

#### email

A WIP project shouldn't be sending people email en masse, the most important considerations are where the information that goes into the email is sourced.  Email could be one of several

#### runners

We should have multiple runners, perhaps simultaneously from different places, that can be configured alongside each other.  At least, we should have Jenkins (since we already wrote it), a local command runner, and a git-based runner (i.e. GitHub Actions/GitLab CI).

#### result submission

We should be able to send checks to Patchwork without waiting for everything to complete, letting the quick things go up fast and everything else can come later.

#### google "<thing I'm trying to do> rust idiom"

Since I'm tearing the whole thing might as well try and set a good example.

#### minimise unnecessary crates

Obviously "necessary" is subjective, but we only want to include crates that we get good value from.  It's very easy to balloon the amount of things we include and as we've seen from previous dependencies, a lot of them bitrot.  Let's focus on a few things with a solid track record.

#### tests

For once I'm actually going to write tests as I go, I swear.
