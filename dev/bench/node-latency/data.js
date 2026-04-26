window.BENCHMARK_DATA = {
  "lastUpdate": 1777222717082,
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
        "date": 1777222717051,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "node/iroh/cold-connect",
            "value": 98392.98330000004,
            "unit": "us"
          },
          {
            "name": "node/native/cold-connect",
            "value": 1513.0580000000464,
            "unit": "us"
          },
          {
            "name": "node/iroh/warm-request",
            "value": 712.4479600000086,
            "unit": "us"
          },
          {
            "name": "node/native/warm-request",
            "value": 802.348120000006,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x8",
            "value": 2932.280800000044,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x8",
            "value": 2810.464200000024,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x32",
            "value": 8582.328640000014,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x32",
            "value": 7042.805199999948,
            "unit": "us"
          },
          {
            "name": "node/iroh/serve-rps",
            "value": 591.7804000000615,
            "unit": "us"
          },
          {
            "name": "node/native/serve-rps",
            "value": 241.72811999997066,
            "unit": "us"
          }
        ]
      }
    ]
  }
}