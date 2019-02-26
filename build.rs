//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2019 IBM Corporation
// Authors:
//     Andrew Donnellan <andrew.donnellan@au1.ibm.com>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// build.rs - snowpatch build script
//

extern crate vergen;

use vergen::{generate_cargo_keys, ConstantsFlags};

fn main() {
    let mut flags = ConstantsFlags::all();
    flags.toggle(ConstantsFlags::SEMVER_FROM_CARGO_PKG);
    generate_cargo_keys(ConstantsFlags::all()).expect("Unable to generate the cargo keys!");
}
