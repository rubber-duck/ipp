# GUI kit composites

Import these components from `@ipp/react/gui-kit`. Declare a `GuiKit` with an explicit skin and the receiving World's contract and fonts. Both bundled skins provide the button, frame, text, progress and accent recipes used by the composites. The mounted [component gallery](../../../../tests/gui/fixtures/component-gallery.tsx) shows inputs and callbacks with real Canvas entities. The [editing guide](EDITING.md) covers native text editing, labels, application tokens and platform file selection.

## Selection and navigation

`Toggle` exposes a button's pressed state through `pressed` or `defaultPressed`. `ToggleGroup` accepts keyed options, a list of selected keys, and `multiple` and `allowEmpty` policies. Only present options appear selected. Native groups supply one Tab entry and directional navigation for eligible items. `Toolbar` takes an orientation and an optional semantic name. A `Pagination` page is one-based. Previous and Next stop at the ends. Zero items renders an empty window. A change to the count clamps the displayed page to the available range. Activation reports the requested page.

`Rating` divides each mark by `precision`, which must evenly divide one. Pointer activation rounds up to the enclosing step. Arrows step, End chooses the maximum, and Home chooses zero when clearing is enabled. Hover previews stay local. `readOnly` and `disabled` suppress changes. For application-controlled values, supply the accepted proposal through the value prop. A refused proposal leaves the displayed value with its owner.

## Dates and times

Use Gregorian civil dates in `YYYY-MM-DD` form, from year 0001 through 9999. No conversion to a machine timezone occurs. Calendar headings and the default first weekday use the supplied locale. Set `weekStartsOn` explicitly to override it. Environments without locale week information default to Monday. A range has a start date and an optional end. The first pick starts a range, the second completes it in chronological order, and the next starts another range. Eligibility applies to its endpoints. Month and year controls support pointer and keyboard activation. `min`, `max` and `isDateDisabled` constrain picks.

`DateInput` and `TimeInput` keep a draft across their native text fields. Locale selects the segment order and the time period labels. Paste a complete ISO date or a 24-hour time into a segment to populate all the parts. Apply or Enter commits a valid draft. Cancel restores the committed value, while Clear reports null on optional inputs. `TimeInput` uses local wall-clock strings in `HH:mm` or `HH:mm:ss` form. Its bounds are inclusive and step alignment starts at the minimum. Noon and midnight retain distinct 24-hour values when the display uses day periods.

`DatePicker` composes the field and a calendar in a light overlay. A single pick or a completed range closes it. Dismissing a range after its first pick retains the open-ended range. Applications can own the open state, selected date or range, and displayed month independently.

## Images, measurements and feeds

`Avatar` takes a registered React asset reference, an accessible name and an optional fixed size. It shows the ready bitmap or the supplied fallback text. Without custom fallback text it uses initials, then a question mark. `decorative` omits its semantic name. The outer entity and its dimensions survive source replacement.

`ImageCropper` takes the original bitmap dimensions and returns its rectangle in source pixels. The preview letterboxes the source while preserving its aspect. Drag inside the rectangle to pan or drag its lower-right corner to resize. Arrows pan by one source pixel and Shift+arrows resize. Zoom retains the center where source bounds allow it. Reset restores the initial constrained rectangle. Source replacement retires the previous gesture. The application owns export.

`Meter` publishes a read-only bounded measurement through `GuiMeter`. It clamps a finite input to its range, rejects invalid ranges and thresholds, and classifies the displayed value relative to the band containing the optimum. The agent API exposes minimum, maximum and current value. A meter's semantic action list is empty.

`Timeline` retains keyed event rows in application order. Titles, timestamps and optional bodies and actions belong to each item. The last event ends the connector. `Waterfall` accepts a finite start and nonnegative duration for each uniquely keyed span. Every row uses the same time scale, including when a subtree is collapsed. Zero-duration spans remain visible as a mark. Expansion and selection use span keys.

## Layout and overlays

`Resizable` fits the current keyed sizes into the available axis after subtracting handles. It distributes excess or deficit equally among panes that can still change. If the minima exceed the extent they shrink proportionally. Space beyond all maxima remains unused. Drag a divider to resize its neighbors or use the arrows, Home and End. Enter collapses or restores a collapsible pane. Applications can seed persisted sizes before mount through `defaultSizes`.

`Carousel` retains its keyed slides and exposes the selected slide through `value`. Its separate swipe grip leaves controls inside the active slide available. Arrow keys, Home, End and navigation buttons choose a slide. During navigation a native animation positions the strip. When the active key disappears, the first available slide is shown.

`Collapsible` retains the body and its authored state by default. Closed body controls become ineligible. Set `unmountOnClose` to release the body's declarations. `Accordion` adds native header navigation and a choice of single or multiple expansion. Item keys preserve identity across reorder. Native height transitions follow measured content and the active motion preference.

Declare `Drawer` as a top-level canvas overlay. The edge selects its placement. Snap points are positive, strictly increasing lengths in logical Canvas units. Its separate drag grip chooses a snap on release or dismisses when released below half the smallest snap. The body can contain an independent scroll area. Keyboard arrows along the opening axis choose adjacent snaps. The native overlay supplies modal or light input routing and dismissal.

## Preferences

Create one settings store with `createGuiPreferences(defaults, skins, storage)` and pass it to `Preferences`. The store reads saved settings before first render. Unsupported skin names and malformed values fall back to defaults. Storage exceptions can be reported with `onError`. The active ready skin stays visible during asynchronous replacement. Each store can select its own skin. Applications can supply storage and platform-motion adapters directly. `browserPreferenceStorage(window, key)` and `browserMotionPreference(window)` opt into browser integration.

Explicit reduce/full settings publish a new World preference when changed. System mode follows the supplied platform source. Without a platform source it observes the World. The mounted declaring skin samples changes from other World writers through one shared inspection reader per client every 250 ms. Unmount releases subscriptions and leaves the committed World preference in place. Native controllers and kit motions both honor reduced motion.
