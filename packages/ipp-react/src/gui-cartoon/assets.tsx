import { Entity } from "../components.js";
import { tabAsset } from "./tab-paint.js";
import { noticeIconAsset } from "./notice-icons.js";
import { meterAsset } from "./meter-paint.js";

export const cartoonAssets = (scope: string) => (
  <Entity id={`${scope}/materials`}>
    {tabAsset(scope)}
    {meterAsset(scope)}
    {noticeIconAsset(scope)}
  </Entity>
);
