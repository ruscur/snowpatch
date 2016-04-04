snowpatch - CI for patches
==========================

[![Build Status](https://travis-ci.org/ruscur/snowpatch.svg?branch=master)](https://travis-ci.org/ruscur/snowpatch)

Overview
-------

snowpatch is a continuous integration tool for projects using a patch-based,
mailing-list-centric git workflow. This workflow is used by a number of
well-known open source projects such as the Linux kernel.

snowpatch serves as a bridge between a patch tracking system and a continuous
integration automation server. snowpatch monitors the patch queue for incoming
patches, applies patches on top of an existing tree, triggers appropriate
builds and test suites, and reports the results.

At present, snowpatch supports
[patchwork-freedesktop](http://github.com/dlespiau/patchwork) and
[Jenkins](http://jenkins-ci.org).

snowpatch is named in honour of
[skiboot](https://github.com/open-power/skiboot), the project which inspired the
creation of snowpatch.


Installing
----------

snowpatch is a [Rust](https://www.rust-lang.org) program.  In order to compile 
it, you will need Rust and its package manager, Cargo.  snowpatch should run 
on any target that Rust compiles on, however it has only been tested on Linux.
We do not provide pre-built binaries at this stage.

snowpatch also requires the `git` binary to be installed.  We try to use the 
[git2-rs](https://github.com/alexcrichton/git2-rs) library where possible,
however some operations require the binary (such as applying patches).

snowpatch can be compiled with `cargo build --release`, which will make
`target/release/snowpatch`.


Contributing
------------

Contributions should be sent as patches to the mailing list (see
Contact below), using `git send-email`.

All commit messages must include a Signed-off-by: line, indicating
that you have read and agree to the
[Developer Certificate of Origin v1.1](http://developercertificate.org).

Please read the Linux kernel
[SubmittingPatches](https://www.kernel.org/doc/Documentation/SubmittingPatches)
guidance before contributing. While many of the rules in there
obviously aren't relevant to snowpatch, a lot of the guidance is
relevant.


Contact
------

snowpatch development is done on our mailing list,
[snowpatch@lists.ozlabs.org](mailto:snowpatch@lists.ozlabs.org). To
subscribe, go to the
[listinfo page](https://lists.ozlabs.org/listinfo/snowpatch).

snowpatch is maintained by
[Russell Currey](mailto:ruscur@russell.cc) and
[Andrew Donnellan](mailto:andrew.donnellan@au1.ibm.com).


License
-------
Copyright © 2016 IBM Corporation.

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
