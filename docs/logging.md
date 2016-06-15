== Logging ==

snowpatch uses [env_logger](https://rust-lang-nursery.github.io/log/env_logger/index.html) to print information for the user.  By default, snowpatch will log everything at the "info" level, which is suitable for general use by non-snowpatch developers.  However, users may want to see less from snowpatch, and only have output when something has gone wrong.  On the other hand, users may want more information about why snowpatch is malfunctioning, or may desire more input while developing snowpatch itself.

To specify the level of logging from snowpatch, set the RUST_LOG environment variable.  snowpatch uses the default env_logger environment variable, since it's easily searchable and it makes it obvious that the logging format is nothing specific to snowpatch.  Refer to the env_logger documentation for more details on that format; some examples are provided below.

To log as much as possible, from snowpatch and libraries it uses:

	`RUST_LOG=trace`

As much as possible, but just from snowpatch itself:

	`RUST_LOG=snowpatch=trace`

Just the snowpatch defaults, and nothing from its libraries:

	`RUST_LOG=snowpatch`

Only print things that may require human attention:

	`RUST_LOG=warn`
