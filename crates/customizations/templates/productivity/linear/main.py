"""Linear tool — manage issues and projects via Linear GraphQL API.

Requires LINEAR_API_KEY (personal API key).
"""
import json
import os
import sys
import urllib.request
import urllib.error


API_URL = "https://api.linear.app/graphql"


def get_headers():
    token = os.environ.get("LINEAR_API_KEY", "")
    if not token:
        return None
    return {"Authorization": token, "Content-Type": "application/json"}


def graphql(query, variables=None):
    headers = get_headers()
    if not headers:
        return {"error": "LINEAR_API_KEY not set"}

    payload = json.dumps({"query": query, "variables": variables or {}}).encode()
    req = urllib.request.Request(API_URL, data=payload, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return {"error": f"Linear API error {e.code}: {e.read().decode()}"}


def list_issues(team_id=""):
    filter_clause = ""
    variables = {}
    if team_id:
        filter_clause = "(filter: { team: { id: { eq: $teamId } } })"
        variables["teamId"] = team_id

    query = f"""
    query($teamId: String) {{
      issues{filter_clause}(first: 20, orderBy: updatedAt) {{
        nodes {{
          id identifier title state {{ name }} assignee {{ name }} updatedAt
        }}
      }}
    }}
    """
    result = graphql(query, variables)
    if "error" in result:
        return result
    issues = [
        {
            "id": n["id"],
            "identifier": n["identifier"],
            "title": n["title"],
            "state": n.get("state", {}).get("name", ""),
            "assignee": (n.get("assignee") or {}).get("name", "Unassigned"),
        }
        for n in result.get("data", {}).get("issues", {}).get("nodes", [])
    ]
    return {"ok": True, "count": len(issues), "issues": issues}


def create_issue(title, description="", team_id=""):
    if not team_id:
        return {"error": "team_id required for create"}

    query = """
    mutation($title: String!, $description: String, $teamId: String!) {
      issueCreate(input: { title: $title, description: $description, teamId: $teamId }) {
        success
        issue { id identifier title url }
      }
    }
    """
    result = graphql(query, {"title": title, "description": description, "teamId": team_id})
    if "error" in result:
        return result
    issue_data = result.get("data", {}).get("issueCreate", {})
    if issue_data.get("success"):
        issue = issue_data.get("issue", {})
        return {"ok": True, "id": issue.get("id", ""), "identifier": issue.get("identifier", ""), "url": issue.get("url", "")}
    return {"error": "Failed to create issue"}


def search_issues(query_str):
    query = """
    query($query: String!) {
      issueSearch(query: $query, first: 20) {
        nodes { id identifier title state { name } }
      }
    }
    """
    result = graphql(query, {"query": query_str})
    if "error" in result:
        return result
    issues = [
        {"id": n["id"], "identifier": n["identifier"], "title": n["title"], "state": n.get("state", {}).get("name", "")}
        for n in result.get("data", {}).get("issueSearch", {}).get("nodes", [])
    ]
    return {"ok": True, "count": len(issues), "issues": issues}


def main():
    data = json.load(sys.stdin)
    action = data.get("action", "")

    if action == "list":
        result = list_issues(data.get("team_id", ""))
    elif action == "create":
        result = create_issue(data.get("title", ""), data.get("description", ""), data.get("team_id", ""))
    elif action == "search":
        result = search_issues(data.get("query", ""))
    elif action == "test":
        headers = get_headers()
        if headers:
            result = {"ok": True, "service": "Linear"}
        else:
            result = {"error": "LINEAR_API_KEY not set"}
    else:
        result = {"error": f"Unknown action: {action}. Use: list, create, search, test"}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
