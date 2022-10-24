Syncron CLI
===========

Synopsis
--------

    syncron --help
    syncron -c <job-cmd>
    syncron [-h] [-v...] exec (-n <name> | -i <id> | -n <name> -i <id>) [--timeout=<timespec>] [--server=<server-url>] <job-cmd>
    syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]

Description
-----------

Syncron is designed to collect logs for periodic jobs and store them in a
central place for easy access via a web based UI.

Common Options
--------------

`-h`, `--help`

: Show usage.

`-v`, `--verbose`

: Be more verbose. This command is additive and will increase the verbosity
  each time it given (eg: `-vvvv` will show lots of internal debugging
  information).

Server Mode
-----------

    syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]

This starts the syncron server. It will listen on the specified port and
write its data into the specified db directory path.

Server Options
--------------

`--db=<path-to-db>`, (env: `SYNCRON_DB`)

: Path to the db. Will be created if it doesn't exist. Defaults to
  `./db`.

`--port=<port>`, (env: `SYNCRON_PORT`)

: Port to listen on. Defaults to `8000`.
  `SYNCRON_PORT` environment variable.

Client Mode
-----------

    syncron -c <job-cmd>
    syncron [-h] [-v...] exec (-n <name> | -i <id> | -n <name> -i <id>) [--timeout=<timespec>] [--server=<server-url>] <job-cmd>

Both of these forms will start a job and write the stderr/stdout to the
server. If the server cannot be reached then the client will not capture
stderr/stdout and instead let it pass through normally. This is so a job's
output is not silently lost if the server is down or otherwise unavailable.

The top form mimics a shell enough for Syncron to stand in for one in a
crontab. See [Adding Jobs](docs/adding-jobs.md) for more info. When using
this form the following enviroment variables are mandatory:

  - `SYNCRON_SERVER`
  - Either `SYNCRON_NAME` or `SYNCRON_JOB_ID`

The bottom form can used to specify everything using command line arguments
instead of environment variables. This could be useful in use cases outside
crontab -- despite the name, Syncron doesn't care if it was launched from
cron or in some other manner.

Client Options
--------------

`-c <job-cmd>`

: Shell combatible equivalent of `syncron exec <job-cmd>`

`-n <name>`, `--name=<name>`, (env: `SYNCRON_NAME`)

: Job name. This can be anything--whitespace and symbols are all fair
  game. This only sets the actual job name on the server if the job doesn't
  already exist. Otherwise it's just used to compute the job id (see below).

`-i <job-id>`, `--id=<job-id>`, (env: `SYNCRON_JOB_ID`)

: Job id. If not specifed, it will be created by "sluggifying" the name: `My
  $$wierdo$$ nAmE! (1234)` becomes `my-weirdo-name-1234` (only ascii
  alphanumerics come through (lowercased), everything else turns into `-`
  and consecutive dashes get coalesced).

`--timeout=<timespec>`

: Time out job if it runs too long. Timespec is `1s`, `3m`, `4h`, etc.

`--server=<server-url>`, (env: `SYNCRON_SERVER`)

: Base URL of a `syncron serve` instance (eg: `http://127.0.0.1:8000`)
