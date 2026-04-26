window.BENCHMARK_DATA = {
  "lastUpdate": 1777222735318,
  "repoUrl": "https://github.com/Momics/iroh-http",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "name": "Momics",
            "username": "Momics",
            "email": "momics.eu@gmail.com"
          },
          "committer": {
            "name": "Momics",
            "username": "Momics",
            "email": "momics.eu@gmail.com"
          },
          "id": "6ad4bf15f457e74774bc6a5636531648b3e98d24",
          "message": "ci(bench): fix gh-pages push — add auto-push and skip-fetch\n\nThe second benchmark-action step in each job tried to git-fetch\ngh-pages after the first step had already committed to it locally,\ncausing a non-fast-forward rejection.\n\nFix: second step skips the fetch (reuses step 1's local branch) and\nauto-pushes both commits.  Add contents:write permission so the\nGITHUB_TOKEN can push to gh-pages.",
          "timestamp": "2026-04-26T16:54:40Z",
          "url": "https://github.com/Momics/iroh-http/commit/6ad4bf15f457e74774bc6a5636531648b3e98d24"
        },
        "date": 1777222735275,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "deno/iroh/cold-connect",
            "value": 1016583.069,
            "unit": "us"
          },
          {
            "name": "deno/native/cold-connect",
            "value": 780.2690999998958,
            "unit": "us"
          },
          {
            "name": "deno/iroh/warm-request",
            "value": 754.8806000000332,
            "unit": "us"
          },
          {
            "name": "deno/native/warm-request",
            "value": 631.9752800000424,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x8",
            "value": 2364.0190799999255,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x8",
            "value": 969.983599999905,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x32",
            "value": 7111.10700000012,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x32",
            "value": 2525.319240000026,
            "unit": "us"
          },
          {
            "name": "deno/iroh/serve-rps",
            "value": 567.0551999997406,
            "unit": "us"
          },
          {
            "name": "deno/native/serve-rps",
            "value": 849.5025599999644,
            "unit": "us"
          }
        ]
      }
    ]
  }
}