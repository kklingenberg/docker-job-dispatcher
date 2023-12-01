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

## Concurrency control using polling

The dispatcher doesn't deal with queues, but a rudimentary mechanism is included
to control the maximum number of concurrent containers being executed. It works
by polling the Docker API for running containers, and selecting the oldest
not-yet-started ones for scheduling. This behaviour is disabled by default,
which implies that no limit is imposed on the number of active jobs.
