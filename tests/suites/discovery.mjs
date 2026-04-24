/**
 * Discovery tests — browse(), pathChanges(), advertise() API surface.
 *
 * These test the API shape (returns AsyncIterable / Promise), not actual mDNS
 * discovery which requires multicast UDP and is unreliable in CI.
 *
 * Shared across all runtimes.
 */

export function discoveryTests({ createNode, test, assert }) {
  test("browse() returns an AsyncIterable", async () => {
    const node = await createNode({ disableNetworking: true });
    try {
      const iterable = node.browse();
      assert(
        typeof iterable[Symbol.asyncIterator] === "function",
        "browse() must return an AsyncIterable",
      );
    } finally {
      await node.close();
    }
  });

  test("pathChanges() returns an AsyncIterable", async () => {
    const node = await createNode({ disableNetworking: true });
    try {
      const iterable = node.pathChanges(node.publicKey);
      assert(
        typeof iterable[Symbol.asyncIterator] === "function",
        "pathChanges() must return an AsyncIterable",
      );
    } finally {
      await node.close();
    }
  });

  test("advertise() resolves when signal is aborted", async () => {
    const node = await createNode();
    try {
      const ac = new AbortController();
      const p = node.advertise({ signal: ac.signal });
      ac.abort();
      await p;
    } finally {
      await node.close();
    }
  });
}
