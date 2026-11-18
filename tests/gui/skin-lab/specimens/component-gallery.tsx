import { GuiKit } from "@ipp/react/gui-kit";
import { ComponentGallery } from "../../fixtures/component-gallery.js";
import { defineSpecimen } from "../specimen.js";
export default defineSpecimen({
  extent: [360, 280],
  states: [{ name: "selection-and-measurement", cell: [0, 0, 360, 280] }],
  render: () => (
    <GuiKit fontSize={10}>
      <ComponentGallery section="selection" />
    </GuiKit>
  ),
});
