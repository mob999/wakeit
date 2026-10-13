#!/usr/bin/env python3
"""Example HTTP adapter. Change routes/field mapping for your Agent service.

Expected REST contract:
GET /models -> {models:[{id,name,parameters}]};
POST /jobs {prompt,parameters} -> {job_id}; GET /jobs/:id -> {state,message?,result?};
POST /jobs/:id/cancel -> any JSON/empty success response.
Only the wakeit JSON contract goes to stdout; diagnostics go to stderr.
"""
import json
import os
import sys
import urllib.parse
import urllib.request


def request(method, path, body=None):
    url = os.environ["SERVICE_URL"].rstrip("/") + path
    headers = {
        "Content-Type": "application/json",
        "Authorization": "Bearer " + os.environ["SERVICE_TOKEN"],
    }
    if method == "POST" and path == "/jobs":
        # The external service must honor this key to make submission idempotent.
        headers["Idempotency-Key"] = os.environ["WAKEIT_RUN_ID"]
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=20) as response:
        raw = response.read(1_000_001)
    if len(raw) > 1_000_000:
        raise ValueError("external response exceeds the adapter limit")
    return json.loads(raw) if raw else {}


def main():
    operation = sys.argv[1]
    if operation == "models":
        response = request("GET", "/models")
        if not isinstance(response.get("models"), list):
            raise ValueError("models response must contain a list")
        result = {"version": 1, "models": response["models"]}
    elif operation == "submit":
        parameters = {}
        args = sys.argv[2:]
        for index in range(0, len(args), 2):
            if args[index] not in {"--model", "--provider", "--reasoning"} or index + 1 >= len(args):
                raise ValueError("invalid submit parameter")
            parameters[args[index][2:]] = args[index + 1]
        response = request("POST", "/jobs", {"prompt": sys.stdin.read(), "parameters": parameters})
        if not isinstance(response.get("job_id"), str) or not response["job_id"]:
            raise ValueError("submit response must contain a nonempty job_id")
        result = {"job_id": response["job_id"]}
    else:
        path = "/jobs/" + urllib.parse.quote(sys.argv[2], safe="")
        if operation == "status":
            response = request("GET", path)
            if response.get("state") not in {
                "queued", "running", "succeeded", "failed", "cancelled"
            }:
                raise ValueError("unrecognized remote state")
            result = {key: response[key] for key in ["state", "message", "result"] if key in response}
        elif operation == "cancel":
            request("POST", path + "/cancel", {})
            result = {"requested": True}
        else:
            raise ValueError("operation must be models, submit, status or cancel")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"adapter failed: {type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(1)
