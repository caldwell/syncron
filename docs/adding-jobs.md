Adding Jobs
===========

Syncron was designed to make it extremely easy to set up in your crontab. It
can act as a shell, which means the configuration can be mostly global.

Let's consider this example crontab:

```
*/2 * * * * echo hi; sleep 1; echo there
*/5 * * * * echo Happens every 5 minutes
```

Add Syncron by setting the `SHELL` and `SYNCRON_SERVER` environment
variables and set `SYNCRON_NAME` for each job you have:

```
SHELL=/path/to/syncron
SYNCRON_SERVER=http://localhost:1234

SYNCRON_NAME="My great cron job"
*/2 * * * * echo hi; sleep 1; echo there

SYNCRON_NAME="My other great cronjob"
*/5 * * * * echo Happens every 5 minutes
```

By default Syncron will run your cron jobs with `/bin/sh`. You can specify
the shell with the `SYNCRON_SHELL` environment variable:

```
SHELL=/path/to/syncron
SYNCRON_SERVER=http://localhost:1234
SYNCRON_SHELL=/bin/bash

SYNCRON_NAME="My great cron job"
*/2 * * * * echo $((1+3))
```

If you just want to run Syncron on a specific set of commands, you can set
the `SHELL` command before and after:

```
SHELL=/path/to/syncron
SYNCRON_SERVER=http://localhost:1234

SYNCRON_NAME="My great cron job"
*/2 * * * * echo hi; sleep 1; echo there

SHELL=/bin/sh
*/5 * * * * echo Happens every 5 minutes
```

...or, run Syncron explicitly:

```
*/2 * * * * /path/to/syncron --server=http://localhost:1234 --name "My great cron job" "echo hi; sleep 1; echo there"

*/5 * * * * echo Happens every 5 minutes
```

This way isn't recommended as it's very easy for the quoting to get out of
hand on any command that's not simple.

## Job Names

Jobs can be named anything (whitespace and symbols are all fair
game). Syncron creates an internal id for the job by "sluggifying" the name:
`My $$wierdo$$ nAmE! (1234)` becomes `my-weirdo-name-1234` (only ascii
alphanumerics come through (lowercased), everything else turns into `-` and
consecutive dashes get coalesced).

This can become an issue if two job names differ only in capitalization,
symbols, or whitespace. It is suggested to just make the names sufficiently
different but if you are dead set on your naming conventions, you can set
the id explicitly with the `SYNCRON_ID` environment variable (or `--id`
flag).
