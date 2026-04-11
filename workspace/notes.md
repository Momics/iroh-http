
  - We are currently writing integration and unit tests for the core and all platforms.
  - Patch 17 is not yet integrated and also needs to be evaluated for 'use'.
  - We should invest heavily in developer UX and great JSDoc like the previous packages. We should have an agent check all developer facing (and perhaps even our own internal code) for exceptional IDE documentation.
  - Much of the core broke when we tried testing which indicates that the developers who built it didn't really write good code. It's vital that an agent 'aggressively' checks all the core code and makes sure it satisfies robust 'Rust-standard' code requirements.
  - Verify that all patches were applied correctly.