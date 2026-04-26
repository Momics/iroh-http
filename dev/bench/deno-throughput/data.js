window.BENCHMARK_DATA = {
  "lastUpdate": 1777222734952,
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
        "date": 1777222734693,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "deno/iroh/throughput-1kb",
            "value": 1.464413271074916,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-1kb",
            "value": 1.4928360075477904,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-64kb",
            "value": 44.68475356780336,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-64kb",
            "value": 81.67720707754737,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-1mb",
            "value": 71.04023162616411,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-1mb",
            "value": 525.0599786515052,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-10mb",
            "value": 87.56225334720217,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-10mb",
            "value": 989.0855301920124,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}