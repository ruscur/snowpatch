//
// snowpatch - continuous integration for patch-based workflows
//
// Copyright (C) 2016 IBM Corporation
// Authors:
//     Russell Currey <ruscur@russell.cc>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// utils.rs - snowpatch generic helpers
//

pub fn sanitise_path(path: String) -> String {
    path.replace("/", "_").replace("\\", "_").replace(".", "_")
        .replace("~", "_").replace(" ", "_").replace(":", "")
        .replace("[", "_").replace("]", "_").replace("'", "")
        .replace("\"", "").replace("(", "_").replace(")", "_")
}
