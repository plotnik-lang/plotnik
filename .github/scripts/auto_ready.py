#!/usr/bin/env python3
"""Promote project items to Ready when an issue's blockers all close.

Runs after an issue closes (GitHub `issues: closed`). For every issue the closed
one was blocking, if all of that issue's blockers are now closed and its board
Status is Backlog, set it to Ready. In progress / Done / Ready are never touched.

The "issue closed -> Done" leg is handled by the project's built-in workflow; this
fills the dependency-driven "blocker closed -> Ready" leg, which Projects has no
built-in for.

Everything goes through the GraphQL API directly (not the `gh` CLI), so it does
not depend on the runner's bundled gh version. Set DRY_RUN=1 to log decisions
without changing the board.

Required environment:
  GH_TOKEN          PAT with org Projects (read & write) + repo Issues (read)
  OWNER, REPO_NAME  repository coordinates
  CLOSED_ISSUE      the issue that just closed
  PROJECT_ID        ProjectV2 node id
  STATUS_FIELD_ID   Status single-select field id
  READY_OPTION      option id for "Ready"
"""

import json
import os
import urllib.request

GRAPHQL = "https://api.github.com/graphql"


def graphql(query, **variables):
    body = json.dumps({"query": query, "variables": variables}).encode()
    request = urllib.request.Request(
        GRAPHQL,
        data=body,
        headers={
            "Authorization": f"bearer {os.environ['GH_TOKEN']}",
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(request) as response:
        payload = json.load(response)
    if payload.get("errors"):
        raise SystemExit("GraphQL error:\n" + json.dumps(payload["errors"], indent=2))
    return payload["data"]


def blocked_by_closed(owner, name, number):
    """Issue numbers that the just-closed issue was blocking."""
    data = graphql(
        """
        query($owner:String!, $name:String!, $n:Int!){
          repository(owner:$owner, name:$name){
            issue(number:$n){ blocking(first:100){ nodes{ number } } }
          }
        }""",
        owner=owner, name=name, n=number,
    )
    return [node["number"] for node in data["repository"]["issue"]["blocking"]["nodes"]]


def dependent_state(owner, name, number, project_id):
    """(state, open_blocker_count, item_id, board_status) for one issue.

    item_id / board_status describe its card on the given project (None if absent).
    """
    data = graphql(
        """
        query($owner:String!, $name:String!, $n:Int!){
          repository(owner:$owner, name:$name){
            issue(number:$n){
              state
              blockedBy(first:100){ nodes{ state } }
              projectItems(first:20){
                nodes{
                  id
                  project{ id }
                  fieldValueByName(name:"Status"){
                    ... on ProjectV2ItemFieldSingleSelectValue { name }
                  }
                }
              }
            }
          }
        }""",
        owner=owner, name=name, n=number,
    )
    issue = data["repository"]["issue"]
    open_blockers = sum(b["state"] == "OPEN" for b in issue["blockedBy"]["nodes"])

    item_id = board_status = None
    for item in issue["projectItems"]["nodes"]:
        if item["project"]["id"] == project_id:
            item_id = item["id"]
            value = item.get("fieldValueByName")
            board_status = value["name"] if value else None
            break

    return issue["state"], open_blockers, item_id, board_status


def set_ready(project_id, item_id, field_id, option_id):
    graphql(
        """
        mutation($project:ID!, $item:ID!, $field:ID!, $option:String!){
          updateProjectV2ItemFieldValue(input:{
            projectId:$project, itemId:$item, fieldId:$field,
            value:{ singleSelectOptionId:$option }
          }){ projectV2Item{ id } }
        }""",
        project=project_id, item=item_id, field=field_id, option=option_id,
    )


def main():
    owner = os.environ["OWNER"]
    name = os.environ["REPO_NAME"]
    closed = int(os.environ["CLOSED_ISSUE"])
    project_id = os.environ["PROJECT_ID"]
    field_id = os.environ["STATUS_FIELD_ID"]
    ready_option = os.environ["READY_OPTION"]
    dry_run = bool(os.environ.get("DRY_RUN"))

    print(f"Trigger: issue #{closed} closed" + (" [DRY RUN]" if dry_run else ""))

    dependents = blocked_by_closed(owner, name, closed)
    if not dependents:
        print(f"No issues were blocked by #{closed} — nothing to do.")
        return
    print(f"#{closed} was blocking: " + ", ".join(f"#{d}" for d in dependents))

    for number in dependents:
        state, open_blockers, item_id, board_status = dependent_state(
            owner, name, number, project_id
        )
        if state != "OPEN":
            print(f"#{number}: {state} — skip")
        elif open_blockers:
            print(f"#{number}: still has {open_blockers} open blocker(s) — skip")
        elif item_id is None:
            print(f"#{number}: not on the board — skip")
        elif board_status != "Backlog":
            print(f"#{number}: status is {board_status!r} — leave it")
        elif dry_run:
            print(f"#{number}: fully unblocked and in Backlog -> WOULD set Ready")
        else:
            set_ready(project_id, item_id, field_id, ready_option)
            print(f"#{number}: fully unblocked and in Backlog -> Ready")


if __name__ == "__main__":
    main()
