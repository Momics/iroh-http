window.BENCHMARK_DATA = {
  "lastUpdate": 1777448434793,
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
      },
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
          "id": "62b6b236cb60ee4f069b1507ee26e6fdb6eabd11",
          "message": "fix: update fetch URLs to use dynamic serverId in benchmark scripts",
          "timestamp": "2026-04-29T07:35:37Z",
          "url": "https://github.com/Momics/iroh-http/commit/62b6b236cb60ee4f069b1507ee26e6fdb6eabd11"
        },
        "date": 1777448434760,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "node/iroh/cold-connect",
            "value": 101234.1056,
            "unit": "us"
          },
          {
            "name": "node/native/cold-connect",
            "value": 1442.2863000000461,
            "unit": "us"
          },
          {
            "name": "node/iroh/warm-request",
            "value": 700.4187600000205,
            "unit": "us"
          },
          {
            "name": "node/native/warm-request",
            "value": 852.5855599999886,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x8",
            "value": 2502.2579600000063,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x8",
            "value": 2462.4487999999474,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x32",
            "value": 6845.145359999933,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x32",
            "value": 5243.365319999939,
            "unit": "us"
          },
          {
            "name": "node/iroh/serve-rps",
            "value": 510.7450000000062,
            "unit": "us"
          },
          {
            "name": "node/native/serve-rps",
            "value": 227.39304000002448,
            "unit": "us"
          }
        ]
      }
    ]
  }
}