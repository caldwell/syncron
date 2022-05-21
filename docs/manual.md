Syncron
-------

Welcome to syncron, etc. etc.

## Adding Jobs

Starting from this example crontab:

```
*/2 * * * * echo hi; sleep 1; echo there
*/5 * * * * echo Happens every 5 minutes
```

Add syncron by setting the `SHELL` and `SYNCRON_SERVER` environment
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

