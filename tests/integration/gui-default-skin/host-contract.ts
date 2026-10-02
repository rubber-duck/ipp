/**
 * `@ipp/host-contract` in this scenario's bundle: the skin lab's palette and
 * geometry read the design-language tokens of the runtime under test, so the
 * page loads that runtime's generated module and publishes it as
 * `globalThis.ippHostContract` before it imports the scenario.
 */
type HostContract = typeof import("@ipp/host-contract");

const contract = (globalThis as { ippHostContract?: HostContract })
  .ippHostContract;
if (!contract)
  throw new Error(
    "Publish the runtime's generated module as globalThis.ippHostContract before importing the scenario",
  );

export const GUI_SKIN_TOKENS = contract.GUI_SKIN_TOKENS;
export const components = contract.components;
