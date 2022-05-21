Syncron CLI
===========

Synopsis
--------

    syncron --help
    syncron -c <job-cmd>
    syncron [-h] [-v...] exec -n <name> [-i <id>] [--timeout=<timespec>] [--server=<server-url>] <job-cmd>
    syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]

Description
-----------

Common Options
--------------

    -h --help              Show this message.
    -v --verbose           Be more verbose.

Server Mode
-----------

    syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]

Server Options
--------------

    --db=<path-to-db>      Path to the db. Will be created if it doesn't exist [default: ./db]
                           (env: SYNCRON_DB)
    --port=<port>          Port to listen on [default: 8000] (env: SYNCRON_PORT)

Server Env Vars
---------------

    SYNCRON_DB
    SYNCRON_PORT

Client Mode
-----------

    syncron -c <job-cmd>
    syncron [-h] [-v...] exec -n <name> [-i <id>] [--timeout=<timespec>] [--server=<server-url>] <job-cmd>

Client Options
--------------

    -c <job-cmd>           Shell combatible equivalent of `syncron exec <job-cmd>`
    -n --name=<name>       Job name (env: SYNCRON_NAME)
    -i --id=<job-id>       Job id (will be created from name is not specified) (env: SYNCRON_JOB_ID)
    --timeout=<timespec>   Time out job if it runs too long. Timespec is '1s, 3m, 4h', etc.
    --server=<server-url>  Base URL of a `syncron serve` instance (env: SYNCRON_SERVER)

Client Env Vars
---------------

    SYNCRON_SHELL
    SYNCRON_SERVER
    SYNCRON_NAME
    SYNCRON_JOB_ID
