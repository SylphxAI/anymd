#!/usr/bin/env python3
"""Verify a workflow's short-lived Access grants without changing Money state."""
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.sylphx.com/v1/"


def request(url, token=None, body=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(
        url, data=json.dumps(body).encode() if body is not None else None,
        headers=headers, method="POST" if body is not None else "GET",
    )
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read()
        return response.status, json.loads(data) if data else {}, response.headers.get("x-request-id")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    name = os.environ["ACCESS_NAME"]
    expected = os.environ["EXPECTED_ENVIRONMENT"]
    wrong_env = os.environ["OTHER_ENVIRONMENT"]
    expected_scopes = sorted(os.environ["EXPECTED_SCOPES"].split(","))
    refused = (os.environ.get("EXPECT_REFUSAL") == "true"
               or os.environ["GITHUB_REF"] != os.environ["EXPECTED_REF"])
    url = os.environ["ACTIONS_ID_TOKEN_REQUEST_URL"]
    url += ("&" if "?" in url else "?") + urllib.parse.urlencode({"audience": "sylphx-access"})
    status, oidc, _ = request(url, os.environ["ACTIONS_ID_TOKEN_REQUEST_TOKEN"])
    require(status == 200 and oidc.get("value"), "GitHub did not issue an OIDC token")
    jwt = oidc["value"]
    print("::add-mask::" + jwt)
    receipt = {"declaration": name, "run_id": os.environ.get("GITHUB_RUN_ID"),
               "sha": os.environ.get("GITHUB_SHA"), "exchanges": []}
    principals = []
    keys = []
    for _ in range(1 if refused else 2):
        status, result, request_id = request(API + "access/github/token", body={"name": name, "token": jwt})
        if refused:
            require(status == 403 and not result.get("api_key"), "Undeclared workflow or ref was admitted")
            receipt["refusal"] = {"status": status, "request_id": request_id}
            print(json.dumps(receipt, sort_keys=True))
            return
        require(status == 200 and result.get("api_key"), "Access exchange failed: HTTP " + str(status))
        key = result["api_key"]
        print("::add-mask::" + key)
        metadata = result["key"]
        require(metadata["name"].rsplit("/api_keys/", 1)[0] == expected, "Access returned the wrong environment")
        require(sorted(metadata["spec"]["scopes"]) == expected_scopes, "Access returned different permissions")
        principal = metadata["status"]["principal"]
        require(principal, "Access returned no workload principal")
        principals.append(principal)
        keys.append(metadata["name"])
        receipt["exchanges"].append({"status": status, "request_id": request_id,
                                     "key": metadata["name"], "principal": principal,
                                     "expires_at": result["expires_at"]})
        # Honor the issuer's published propagation bound; no retry or new mint.
        time.sleep(result.get("active_after_ms", 0) / 1000)
        status, _, request_id = request(API + expected + "/price_catalogs/default", key)
        require(status == 200, "Own catalogue read failed: HTTP " + str(status))
        receipt["own_environment_read"] = {"status": status, "request_id": request_id}
        status, _, request_id = request(API + wrong_env + "/price_catalogs/default", key)
        require(status in (403, 404), "Cross-environment catalogue read was admitted")
        receipt["cross_environment_refusal"] = {"status": status, "request_id": request_id}
    require(principals[0] == principals[1], "Workload principal changed during credential rotation")
    require(keys[0] != keys[1], "The exchange did not rotate credentials")
    status, result, request_id = request(API + "access/github/token", body={
        "name": os.environ["OTHER_REPOSITORY_DECLARATION"], "token": jwt})
    require(status == 403 and not result.get("api_key"), "Cross-repository declaration was admitted")
    receipt["cross_repository_refusal"] = {"status": status, "request_id": request_id}
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        # Transport exceptions can contain credentials; report only our fixed
        # validation messages, never a response body, URL, JWT, or exception repr.
        message = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        print("::error::" + message, file=sys.stderr)
        sys.exit(1)
