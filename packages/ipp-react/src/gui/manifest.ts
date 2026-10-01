export const guiComponentContract = {
  GuiScrollView: { host: "ipp-gui-scroll-view", fields: { axis: "number" } },
  GuiVirtualList: {
    host: "ipp-gui-virtual-list",
    fields: {
      item_count: "number",
      item_extent: "number",
      overscan: "number",
      axis: "number",
    },
  },
  GuiVirtualItem: { host: "ipp-gui-virtual-item", fields: { index: "number" } },
  CanvasStyle: {
    host: "ipp-canvas-style",
    fields: {
      x: "number",
      y: "number",
      scale_x: "number",
      scale_y: "number",
      red: "number",
      green: "number",
      blue: "number",
      alpha: "number",
      opacity: "number",
      clipped: "boolean",
      clip_min_x: "number",
      clip_min_y: "number",
      clip_max_x: "number",
      clip_max_y: "number",
    },
  },
  CanvasText: {
    host: "ipp-canvas-text",
    fields: {
      text: "string",
      source: "string",
      variant: "number",
      font_size: "number",
    },
  },
  CanvasGlyphRun: {
    host: "ipp-canvas-glyph-run",
    fields: {
      source: "string",
      variant: "number",
      font_size: "number",
      glyphs: "bytes",
    },
  },
  CanvasDrawing: {
    host: "ipp-canvas-drawing",
    fields: { source: "string", variant: "number" },
  },
  CanvasBitmap: {
    host: "ipp-canvas-bitmap",
    fields: {
      source: "string",
      variant: "number",
      width: "number",
      height: "number",
    },
  },
  CanvasBox: {
    host: "ipp-canvas-box",
    fields: {
      width: "number",
      height: "number",
      radius_x: "number",
      radius_y: "number",
    },
  },
  GuiLayout: {
    host: "ipp-gui-layout",
    fields: {
      kind: "number",
      width: "number",
      height: "number",
      min_width: "number",
      min_height: "number",
      max_width: "number",
      max_height: "number",
      flex: "number",
      align_x: "number",
      align_y: "number",
      clip: "boolean",
      padding_top: "number",
      padding_right: "number",
      padding_bottom: "number",
      padding_left: "number",
      margin_top: "number",
      margin_right: "number",
      margin_bottom: "number",
      margin_left: "number",
    },
  },
  GuiBehavior: {
    host: "ipp-gui-behavior",
    fields: {
      enabled: "boolean",
      visible: "boolean",
      focus_scope: "boolean",
      semantic_label: "string",
    },
  },
  GuiFont: {
    host: "ipp-gui-font",
    fields: { source: "string", variant: "number", font_size: "number" },
  },
  GuiTheme: { host: "ipp-gui-theme", fields: { parts: "bytes" } },
  GuiSkin: {
    host: "ipp-gui-skin",
    fields: { theme: "entity", parts: "bytes" },
  },
  GuiButton: { host: "ipp-gui-button", fields: { label: "string" } },
  GuiCheckbox: {
    host: "ipp-gui-checkbox",
    fields: { label: "string", checked: "boolean" },
  },
  GuiSlider: {
    host: "ipp-gui-slider",
    fields: {
      min: "number",
      max: "number",
      step: "number",
      value: "number",
    },
  },
  GuiTextInput: {
    host: "ipp-gui-text-input",
    fields: { placeholder: "string", text: "string" },
  },
} as const;

export const guiControlNames = new Set([
  "GuiScrollView",
  "GuiVirtualList",
  "GuiButton",
  "GuiCheckbox",
  "GuiSlider",
  "GuiTextInput",
]);
