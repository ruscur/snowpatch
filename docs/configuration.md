Configuration
=============

snowpatch configuration files are in [TOML](https://en.wikipedia.org/wiki/TOML)
format.

Example configuration files can be found in the [examples](../examples)
directory.

A snowpatch configuration file contains three global configuration sections
(tables, in TOML terms), `git`, `patchwork` and `jenkins`, and a `projects`
section containing per-project configuration.


Git Configuration
-----------------

The `git` section contains settings for the git SSH transport.

Example:

```
[git]
user = "git"
public_key = "/home/ruscur/.ssh/id_rsa.pub"
private_key = "/home/ruscur/.ssh/id_rsa"
```

- `user`: git SSH username (Currently, snowpatch only supports a single git
  username for all git remotes - this will be addressed in future.)

- `public_key`: path to SSH public key, usually `~/.ssh/id_rsa.pub` (optional)

- `private_key`: path to SSH private key, usually `~/.ssh/id_rsa`

- `passphrase`: passphrase for SSH private key (optional)

Patchwork Configuration
-----------------------

The `patchwork` section contains settings for the Patchwork instance being
monitored.

Example:

```
[patchwork]
url = "https://russell.cc/patchwork"
port = 443 # optional
user = "ruscur"
pass = "banana"
polling_interval = 10 # polling interval in minutes
```

- `url`: base URL of the Patchwork instance

- `port`: port number (optional)

- `user`: Patchwork username (must be used in conjuction with `pass`)

- `pass`: Patchwork password (must be used in conjuction with `user`)

- `token`: Patchwork API token (can be used instead of `user`/`pass`)

- `polling_interval`: Patchwork polling interval, in minutes


Jenkins Configuration
---------------------

The `jenkins` section contains settings for the Jenkins instance being used for
builds.

Example:

```
[jenkins]
url = "https://jenkins.ozlabs.ibm.com"
port = 443
username = "patchwork"
token = "33333333333333333333333333333333"
```

- `url`: base URL of the Jenkins instance

- `port`: port number (optional)

- `username`: Jenkins username (optional, must be used in conjunction with
  `token`)

- `token`: Jenkins API token (optional, must be used in conjunction with
  `username`)


Project Configuration
---------------------

The `projects` section consists of subsections for each project, which must be
named `project.PROJECT_NAME`, where `PROJECT_NAME` corresponds to the short name
("linkname") of the project in Patchwork.

Within each project subsection, there is an array called `jobs` which consists
of a table for each Jenkins job that should executed.

Example:

```
    [projects.linuxppc-dev]
    repository = "/home/ruscur/Documents/linux"
    branches = ["master", "powerpc-next"]
    # test_all_branches defaults to true
    remote_name = "github"
    remote_uri = "git@github.com:ruscur/linux.git"
    push_results = false

        [[projects.linuxppc-dev.jobs]]
        job = "linux-build-manual"
        remote = "GIT_REPO"
        branch = "GIT_REF"
        artifact = "snowpatch.txt"
        hefty = true
        DEFCONFIG_TO_USE = "pseries_le_defconfig"

        [[projects.linuxppc-dev.jobs]]
        job = "linux-build-manual"
        remote = "GIT_REPO"
        branch = "GIT_REF"
        artifact = "snowpatch.txt"
        hefty = false
        DEFCONFIG_TO_USE = "ppc64le_defconfig"
```

- `repository`: path to local clone of git repository

- `branches`: a list of base branches (as defined by the local git repository)
  that patches should be tested against

- `test_all_branches`: if true, each patch will be tested against all base
  branches. If false, a patch will only be tested against the first base branch
  to which it successfully applies. (Optional, defaults to true)

- `remote_name`: the name of the remote, as defined in the local git repository,
  to which branches should be pushed so that the Jenkins server can pull them

- `remote_uri`: the URI of the remote

- `push_results`: whether test results should be pushed to Patchwork for this project

Individual jobs contain the following:

- `job`: the name of the Jenkins job to run

- `title`: title of the test which will appear in Patchwork (Optional, defaults
  to job name)

- `remote`: the name of the Jenkins build parameter in which the URI of the git
  remote will be filled

- `branch`: the name of the Jenkins build parameter in which the name of the git
  branch to which the patch has been applied will be filled

- `hefty`: whether this job is a "hefty" test. Hefty tests will only be run on
  the final patch of a series, while non-hefty tests will be run on every patch
  in the series. (Optional, defaults to false)

- `warn_on_fail`: if true, this job will return a warning rather than a failure
  if it fails (Optional, defaults to false)

- Any further parameters will be passed to Jenkins as build parameters