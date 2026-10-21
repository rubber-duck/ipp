export function browserLaunchOptions(
  rendering?: boolean,
  options?: {
    htmlInCanvas?: boolean;
    experimentalWebPlatformFeatures?: boolean;
  },
): import("playwright").LaunchOptions;
export const browserPin: {
  channel: string;
  version: string;
  revision: string;
  resolvedAt: string;
  source: string;
};
export function verifyHardwareRenderer(renderer: unknown): void;
