# Docker job dispatcher

This is a minimal API used to dispatch prepared docker containers to act as
jobs. It's an experiment in using [jq](https://jqlang.github.io/jq/) as a
configuration language (much like
[k8s-job-dispatcher](https://github.com/kklingenberg/k8s-job-dispatcher)).

The exposed API transforms requests into Docker API requests for creating and
retrieving containers, and the transformation is executed using jq filters and
the [jaq](https://github.com/01mf02/jaq) library. The jq filters can be
configured by the user, giving them freedom to interpret the requests and
assemble the container manifests.

## Synopsis

```text
Job-dispatching interface acting as a docker container scheduler

Usage: docker-job-dispatcher [OPTIONS] [FILTER]

Arguments:
  [FILTER]  Filter converting requests to container manifests

Options:
  -f, --from-file <FROM_FILE>
          Read filter from a file [env: FROM_FILE=]
  -p, --port <PORT>
          TCP port to listen on [env: PORT=] [default: 8000]
  -m, --max-concurrent <MAX_CONCURRENT>
          Maximum number of concurrently-running containers; default is unlimited; set to 0 to never start jobs [env: MAX_CONCURRENT=]
  -k, --keep-exited-for <KEEP_EXITED_FOR>
          Interval in seconds to keep an exited job; default is to keep them forever [env: KEEP_EXITED_FOR=]
  -u, --upkeep-interval <UPKEEP_INTERVAL>
          Interval in seconds to perform periodic scheduling and cleanup upkeep [env: UPKEEP_INTERVAL=] [default: 3]
  -t, --transport <TRANSPORT>
          Means of connection to the docker daemon [env: TRANSPORT=] [default: socket] [possible values: http, tls, socket]
  -n, --namespace <NAMESPACE>
          Label applied to jobs created to group them [env: NAMESPACE=] [default: default]
      --log-level <LOG_LEVEL>
          Log level [env: LOG_LEVEL=] [default: INFO]
  -h, --help
          Print help
  -V, --version
          Print version

```

## Monitoring

A very basic metric can be queried from the `/metrics` endpoint, which is
exposed in OpenMetrics format. The `action` label corresponds to the docker
system events `create`, `start` and `die`. The following applies: jobs running ≅
jobs started - jobs died. However, started jobs as exposed by the metric doesn't
differentiate between new jobs and job restarts. Also, the `status` label
applies only to died jobs and can be used to distinguish successful jobs from
failed ones. For example:

```text
# HELP jobs Number of jobs.
# TYPE jobs counter
jobs_total{namespace="default",action="die",status="124"} 3
jobs_total{namespace="default",action="create",status=""} 2
jobs_total{namespace="default",action="start",status=""} 4
jobs_total{namespace="default",action="die",status="0"} 1
# EOF
```

This snapshot shows no jobs currently running, 2 different jobs created, one of
them successful and the other one failed with 2 restarts (both also failing).

## Concurrency control using polling

The dispatcher doesn't deal with queues, but a rudimentary mechanism is included
to control the maximum number of concurrent containers being executed. It works
by polling the Docker API for running containers, and selecting the oldest
not-yet-started ones for scheduling. This behaviour is disabled by default,
which implies that no limit is imposed on the number of active jobs.
