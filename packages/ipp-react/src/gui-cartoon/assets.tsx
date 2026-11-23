import { Entity } from "../components.js";
import { cartoonChartAssets } from "./charts.js";
import { tabAsset } from "./tab-paint.js";
import { noticeIconAsset } from "./notice-icons.js";
import { meterAsset } from "./meter-paint.js";

export const cartoonAssets = (scope: string) => (
  <Entity id={`${scope}/materials`}>
    {tabAsset(scope)}
    {meterAsset(scope)}
    {cartoonChartAssets(scope)}
    {noticeIconAsset(scope)}
  </Entity>
);
