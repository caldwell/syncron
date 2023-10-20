Syncron
=======

Syncron is a dashboard for your cron jobs. It collects run information
including environment, timings, and logs from each job and presents them in
an easy to read web application.

Architecture
------------

![Architecture Overview](/docs/architecture.svg)

Syncron consists of a client and a server. The client runs on each machine
you want to collect job data from. It is designed to act as a "passthrough
shell"--you tell cron to use it as your shell (via the `SHELL` environment
variable), and it runs your code collects the output and sends it off to the
server. You can also run it in a client mode that doesn't try to mimic a
shell.

The server is an http server. It acts as both a server for the Syncron web
application to view logs and as the backend api server that the syncron
clients deliver job info to. The server stores job logs on the filesystem
and all other job metadata in an SQLite database.

The client and the server are compiled into the same binary. See [the
syncron cli reference](/docs/cli.md) for more information.

Despite the name and original intent, there is nothing that makes Syncron
particularly tied to crontabs--it can handle any sort of repeating job that
outputs to stdout/stderr.

Installing
----------

The latest Syncron binary is available
[here](https://github.com/caldwell/syncron/releases/latest).  You can put it
anywhere in the filesystem.

You do not need to (and should not) run the Syncron server as
`root`. Instead, create a low privileged user and run Syncron as that. A
typical Syncron server process might be invoked as:

    /path/to/syncron serve -v --port=4567 --db=/path/to/db

The database directory must exist and be writable by the syncron process.
The port is arbitrary. Syncron does not daemonize on its own and logs to
stderr.

To start Syncron automatically on a `systemd` controlled system, put
something like this in `/etc/systemd/system/syncron.service` (assumes a
`syncron` user):

    [Unit]
    Description=Syncron Server
    Wants=network.target

    [Install]
    WantedBy=multi-user.target

    [Service]
    User=syncron
    WorkingDirectory=~
    ExecStart=/path/to/syncron serve -v --port=4567 --db=/path/to/db
    Restart=on-failure
    RestartSec=2s
    StandardOutput=journal
    StandardError=inherit

Building From Source
--------------------
If no binary is available you can build Syncron from the source.

#### Requirements

  - [A recent nightly Rust compiler](https://rustup.rs/)
  - [Node and npm](https://nodejs.org/)
  - Make. I hope this is included in your OS. 🙂

#### Build command

To make a self contained binary in `./target/release/syncron`:

    make release

This will compile the rust binary, install the node modules and package up
the javascript/html bits for the server portion.

To make a self contained debug version in `./target/debug/syncron`:

    make

When developing, usually what you want is just:

    cargo build

This doesn't create a self contained binary. Instead, the front end web code
and documentation will be served from the filesystem (`./web` and `./docs`
respectively). This is usually more convenient when developing the front end
code or documentation since you can just reload the web browser to get any new
changes instead of rebuilding the app.

#### Running tests

    cargo test


Todo
----

Syncron is prerelease code at the moment. I'm personally running it "in
production" on a machine but I don't fully recommend it. I've tried my best
to make it robust, but use at your own risk!

Big features that are not implemented (in no particular order):

- [ ] Progress bars with timing based on previous runs
- [X] A horizontal plot showing good/failed jobs a-la uptime robot
- [ ] Hosts (it currently works fine across hosts, but the namespace is per
      user instead of per host/user pair)
- [ ] Renaming jobs from the web interface
- [ ] Terminal UI a-la tig
- [ ] Pruning old job runs, with configurable retention period
- [ ] Job deletion
- [ ] Authentication (currently anyone with access to the port can do
      anything a client could do)
- [ ] Alerting when important jobs fail. Web hook? Slack Post? Email?

License
-------

Copyright © 2022-2023 David Caldwell <david_syncron@porkrind.org>

*TLDR: [GPLv3](/docs/license.md). You can redistribute the binary (or a
modified version) as long as you ship the source code used to build it
alongside.*

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
