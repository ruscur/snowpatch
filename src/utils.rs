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

pub fn sanitise_path(path: &str) -> String {
    path.replace(|c: char| !c.is_alphanumeric(), "_")
}

#[cfg(test)]
mod test {
    use super::sanitise_path;

    #[test]
    fn test_dereference() {
        assert_eq!(
            sanitise_path("powerpc/64: Simplify __secondary_start paca->kstack handling"),
            "powerpc_64__Simplify___secondary_start_paca__kstack_handling"
        );
    }

    #[test]
    fn test_semi_colon() {
        assert_eq!(
            sanitise_path("powerpc: hwrng; fix missing of_node_put()"),
            "powerpc__hwrng__fix_missing_of_node_put__"
        );
    }

    #[test]
    fn test_null() {
        assert_eq!(
            sanitise_path("powerpc: There is a \0 in this subject"),
            "powerpc__There_is_a___in_this_subject"
        );
    }

    #[test]
    fn test_paths() {
        assert_eq!(sanitise_path("cat /etc/passwd"), "cat__etc_passwd");
    }

    #[test]
    fn test_options() {
        assert_eq!(sanitise_path("rm -rf /"), "rm__rf__");
    }
}
