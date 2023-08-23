snowpatch - CI for patches
==========================

Overview
-------

snowpatch is a continuous integration tool for projects using a patch-based,
mailing-list-centric git workflow. This workflow is used by a number of
well-known open source projects such as the Linux kernel.

snowpatch serves as a bridge between a patch tracking system and a continuous
integration automation server. snowpatch monitors the patch queue for incoming
patches, applies patches on top of an existing tree, triggers appropriate
builds and test suites, and reports the results.

At present, snowpatch only supports
[Patchwork](http://jk.ozlabs.org/projects/patchwork/) as its source of patches 
and state, and GitHub Actions as its test runner.

snowpatch is designed in line with Patchwork's philosophy of supplementing,
rather than replacing, existing workflows. For projects which already use
Patchwork as their core patch management tool, snowpatch does not impose any
additional changes to the development workflow.

snowpatch requires nothing more than a Patchwork account and GitHub account
with appropriate permissions and an API key. Many projects using patch-based
workflows are highly decentralised, using Patchwork instances they do not
administer, and build machines hidden behind corporate firewalls. As such,
snowpatch is designed not to require administrator access.

snowpatch is deliberately minimalistic, which distinguishes it from tools like
[Zuul](https://zuul-ci.org) which are more sophisticated and may be more
appropriate for projects with a better ability to adapt their workflow around a
comprehensive CI system. Other patch tracking systems that provide integrated
support for CI systems, such as
[Patchew](https://github.com/patchew-project/patchew), may also be appropriate
for projects with suitable workflows.

snowpatch is named in honour of
[skiboot](https://github.com/open-power/skiboot), the project which inspired the
creation of snowpatch.


Project Status
--------------

snowpatch is in the "Odd Fixes" state.  The Linux kernel describes this as:

>"It has a maintainer but they don't have time to do much other than throw
>the odd patch in."

snowpatch is still happily running the developer CI for a few projects so it will
continue to get fixes, but substantial new features are unlikely unless someone
(which could be you!) has a surge of time and motivation.

Installing
----------

snowpatch is a [Rust](https://www.rust-lang.org) program.  In order to compile
it, you will need Rust and its package manager, Cargo.  snowpatch should run on
any target that supports Rust and Git, however it has only been tested on Linux.
We do not provide pre-built binaries at this stage.

### Non-Rust dependencies

* [`git`](https://git-scm.com): we try to use the
  [`git2-rs`](https://github.com/alexcrichton/git2-rs) library where
  possible, but we still need the binary for a few operations.
* [OpenSSH](https://www.openssh.com) headers

### Building manually

To compile snowpatch manually, clone the git repository and run `cargo build
--release`. The executable can be found in `target/release/snowpatch` or
executed using `cargo run`.


Documentation
-------------

For usage information, run `snowpatch --help`.

Example configurations can be found in the [`examples`](examples) directory.

Additional documentation can be found in the [`docs`](docs) directory.


Contributing
------------

Open a pull request.

Contact
------

Just do stuff through GitHub issues & pull requests.

snowpatch is maintained by
[Andrew Donnellan](mailto:ajd@linux.ibm.com).


Licence
-------
Copyright © 2016-2023 IBM Corporation.

This program is free software; you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation; either version 2 of the License, or (at your option) any later
version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE.  See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
this program; if not, write to the Free Software Foundation, Inc., 51 Franklin
Street, Fifth Floor, Boston, MA 02110-1301, USA.
