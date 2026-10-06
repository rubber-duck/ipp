import {
  createRef,
  StrictMode,
  Suspense,
  startTransition,
  useState,
} from "react";
import type { GuiWorldClient } from "@ipp/client";
import { createRoot, Entity, Children } from "@ipp/react";
import {
  Layout,
  Box,
  VirtualList,
  ScrollView,
  type GuiControlHandle,
  type GuiScrollPosition,
  type GuiVirtualRange,
} from "@ipp/react/gui";
import { deferred, entity, type GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";

export async function guiScroll(
  client: GuiWorldClient,
  peer: GuiWorldClient,
  contract: GuiContract,
): Promise<void> {
  const ref = createRef<GuiControlHandle>();
  const errors: Error[] = [];
  const ranges: GuiVirtualRange[] = [];
  const scrolls: GuiScrollPosition[] = [];
  let changed = deferred<void>();
  const root = createRoot(client, { onError: (error) => errors.push(error) });
  const receive = (range: GuiVirtualRange) => {
    ranges.push(range);
    const previous = changed;
    changed = deferred<void>();
    previous.resolve();
  };
  const scene = (count = 100_000, extra = false) => (
    <StrictMode>
      <Entity id="ordinary-virtual-list">
        <Layout width={100} height={50} />
        <VirtualList
          ref={ref}
          item_count={count}
          item_extent={10}
          overscan={1}
          axis={1}
          onRangeChange={receive}
          onScroll={(event) => scrolls.push(event.value)}
          renderItem={(index) => (
            <>
              <Layout width={100} height={extra && index === 39 ? 30 : 10} />
              <Box width={100} height={10} />
            </>
          )}
        />
      </Entity>
    </StrictMode>
  );
  const wait = async (predicate: (range: GuiVirtualRange) => boolean) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        (async () => {
          while (!ranges.length || !predicate(ranges.at(-1)!)) {
            if (errors.length) throw errors[0];
            await changed.promise;
          }
        })(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(
                  `Virtual range timeout: ${JSON.stringify(ranges, (_, value) => (typeof value === "bigint" ? String(value) : value))}; errors=${errors}`,
                ),
              ),
            8_000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  };
  try {
    await root.render(scene());
    await wait((range) => range.first === 0 && range.last === 6);
    check(
      ref.current,
      "VirtualList did not publish its acknowledged control ref",
    );
    const result = await ref.current.action({
      kind: "scrollToIndex",
      index: 40,
      offset: 2,
    });
    check(result.ok, "VirtualList scroll-to-index rejected");
    await wait((range) => range.first === 39 && range.last === 47);
    // onScroll reported the current position first, then the new anchor.
    check(
      scrolls[0]?.anchorIndex === 0 && scrolls.at(-1)?.anchorIndex === 40,
      `onScroll did not observe the committed position: ${JSON.stringify(scrolls)}`,
    );
    await root.render(scene(100_000, true));
    await wait((range) => range.content[1] === 1_000_020);
    const measured = await ref.current.read();
    check(
      measured.anchor_index === 40 && measured.offset_y === 422,
      "Measurement above the viewport moved the visible anchor",
    );
    await root.render(scene(8));
    await wait((range) => range.itemCount === 8);
    const clamped = await ref.current.read();
    check(
      clamped.offset_y === 30 && clamped.anchor_index === 3,
      "Shrinking did not commit the surviving clamped anchor",
    );
    await root.render(scene());
    await wait((range) => range.itemCount === 100_000);
    const regrown = await ref.current.read();
    check(
      regrown.offset_y === 30 && regrown.anchor_index === 3,
      "Regrowth resurrected discarded out-of-range intent",
    );
    const old = ref.current;
    await root.render(scene(0));
    await wait(
      (range) => range.itemCount === 0 && range.first === 0 && range.last === 0,
    );
    const empty = await ref.current.read();
    check(
      empty.offset_y === 0,
      "Empty VirtualList retained an unreachable offset",
    );
    await root.render(
      <Entity id="ordinary-scroll-view">
        <Layout
          width={100}
          height={50}
          padding_top={10}
          padding_right={10}
          padding_bottom={10}
          padding_left={10}
        />
        <ScrollView ref={ref} axis={2} onRangeChange={receive} />
        <Children>
          <Entity id="scroll-content">
            <Layout width={300} height={150} />
            <Box width={300} height={150} />
          </Entity>
        </Children>
      </Entity>,
    );
    check(ref.current, "ScrollView omitted its acknowledged ref");
    await wait((range) => range.itemCount === null);
    const viewport = await ref.current.read();
    check(
      viewport.viewport_x === 80 &&
        viewport.viewport_y === 30 &&
        viewport.capacity_x === 220 &&
        viewport.capacity_y === 120,
      "ScrollView did not measure its padded two-axis viewport/content",
    );
    check(
      (await ref.current.action({ kind: "scrollBy", delta: [500, 500] })).ok,
      "ScrollView scrolling rejected",
    );
    const moved = await ref.current.read();
    check(
      moved.offset_x === 220 && moved.offset_y === 120,
      "ScrollView did not clamp both committed axes",
    );
    const setup = await peer.batch([
      contract.Entity.create(1, { symbolicId: "producer-virtual-list" }),
      contract.GuiLayout.insert(contract.Entity.alias(1), {
        width: 100,
        height: 50,
      }),
      contract.GuiVirtualList.insert(contract.Entity.alias(1), {
        item_count: 32,
        item_extent: 10,
        overscan: 1,
      }),
    ]);
    check(setup.ok, "Producer virtual list setup failed");
    const producer = await entity(peer, "producer-virtual-list");
    // Configuration only: layout writes the range and geometry fields.
    const configuration = (
      fields: Readonly<Record<string, unknown>> | undefined,
    ) =>
      JSON.stringify(
        ["item_count", "item_extent", "overscan", "axis"].map(
          (name) => fields?.[name],
        ),
      );
    const originalConfig = configuration(
      producer.components.find(
        (value) => value.component === contract.GuiVirtualList.id,
      )?.fields,
    );
    const bound = (
      <Entity bindTo="producer-virtual-list">
        <VirtualList
          ref={ref}
          onRangeChange={receive}
          renderItem={() => <Layout width={100} height={10} />}
        />
      </Entity>
    );
    await root.render(bound);
    await wait(
      (range) => range.target.entity === producer.id && range.itemCount === 32,
    );
    await root.render(bound);
    const realized = (await peer.inspect()).entities.filter(
      (value) => value.link.parent === producer.id,
    );
    check(
      realized.length === 6,
      `Bound list omitted ACKed producer range: ${realized.length} realized items`,
    );
    await root.unmount();
    const remaining = await entity(peer, "producer-virtual-list");
    check(
      configuration(
        remaining.components.find(
          (value) => value.component === contract.GuiVirtualList.id,
        )?.fields,
      ) === originalConfig,
      "Bound realization reauthored the producer's count/extent configuration",
    );
    // Unmount deletes nothing: the realized items stay until a client
    // deletes them.
    const retained = (await peer.inspect()).entities.filter(
      (value) => value.link.parent === producer.id,
    );
    check(
      retained.length === realized.length,
      "Unmount deleted declared realized children",
    );
    check(
      (
        await peer.batch(
          retained.map((value) =>
            contract.Entity.delete(contract.Entity.handle(value.id)),
          ),
        )
      ).ok,
      "Retained realized children could not be deleted",
    );
    check(ref.current === null, "Unmount retained a VirtualList ref");
    let rejected = false;
    try {
      await old.read();
    } catch {
      rejected = true;
    }
    check(rejected, "Retired VirtualList handle silently retargeted");
    check(errors.length === 0, `VirtualList errors: ${errors}`);
    await speculativeRangeListener(client);
  } finally {
    await root.unmount();
  }
}

async function speculativeRangeListener(client: GuiWorldClient): Promise<void> {
  const ref = createRef<GuiControlHandle>();
  const root = createRoot(client);
  const attempted = deferred<void>();
  const suspended = deferred<void>();
  const ready = deferred<void>();
  const delivered = deferred<string>();
  let speculate: (() => void) | undefined;
  let observing = false;
  const listener = (name: string) => (range: GuiVirtualRange) => {
    if (range.last === 6) ready.resolve();
    if (observing) delivered.resolve(name);
  };
  function Scene() {
    const [speculative, setSpeculative] = useState(false);
    speculate = () => startTransition(() => setSpeculative(true));
    return (
      <Entity id="speculative-range-listener">
        <Layout width={100} height={50} />
        <VirtualList
          ref={ref}
          item_count={100}
          item_extent={10}
          overscan={1}
          onRangeChange={listener(speculative ? "speculative" : "committed")}
          renderItem={() => {
            if (speculative) {
              attempted.resolve();
              throw suspended.promise;
            }
            return <Layout width={100} height={10} />;
          }}
        />
      </Entity>
    );
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      (async () => {
        await root.render(
          <Suspense fallback={null}>
            <Scene />
          </Suspense>,
        );
        await ready.promise;
        await root.flush();
        check(
          speculate && ref.current,
          "Committed list was not ready for speculative render",
        );
        speculate();
        await attempted.promise;
        observing = true;
        const result = await ref.current.action({
          kind: "scrollToIndex",
          index: 20,
          offset: 0,
        });
        check(
          result.ok,
          "Committed list stopped accepting actions during suspended render",
        );
        check(
          (await delivered.promise) === "committed",
          "Abandoned VirtualList render replaced its committed range listener",
        );
      })(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Suspended range-listener test timed out")),
          8_000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
    await root.unmount();
    suspended.resolve();
  }
}
