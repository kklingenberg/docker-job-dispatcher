# This is an illustrative default conversion filter. It implicitly
# defines the request body structure (here: an object with an 'args'
# field and an optional 'id' field).

{
  Name: (($ENV.JOB_NAME_PREFIX // "demo-job-") + (.id // @sha1)),
  Image: $ENV.JOB_IMAGE // "busybox:latest",
  Entrypoint: [$ENV.JOB_COMMAND // "echo"],
  Cmd: .args,
  Env: [
    $ENV
    | to_entries[]
    | select(.key | startswith("JOB_ENV_"))
    | (.key | ltrimstr("JOB_ENV_")) + "=" + .value
  ],
  HostConfig: {
    NetworkMode: $ENV.JOB_NETWORK,
    LogConfig: {
      Type: "json-file",
      Config: {
        "max-size": "10m",
        "max-file": "1"
      }
    },
    RestartPolicy: {
      Name: "on-failure",
      MaximumRetryCount: try ($ENV.JOB_RETRIES | tonumber) catch 2
    }
  }
}
