import { COCKPIT_PANE_COUNT } from "./options.js";
/** The same task-oriented controls are rendered flat for review and on the bridge. */
import { useState, useSyncExternalStore } from "react";
import {
  Button,
  Checkbox,
  Column,
  Dropdown,
  Field,
  Form,
  FormSection,
  LabelledSlider,
  NumericStepper,
  RadioGroup,
  Row,
  SegmentedControl,
  Separator,
  Switch,
  TextLine,
  Toolbar,
  useGuiKit,
} from "@ipp/react/gui-kit";
import type {
  GalleryOptions,
  GallerySceneContext,
} from "../../shared/scene.js";
import { COCKPIT_SCREEN_DISTANCE, type CockpitOptions } from "./options.js";
import { COCKPIT_RENDER_SCALES } from "../../shared/display-settings.js";
import { DEFAULT_DEMO_FONT, DEMO_FONTS, demoFont } from "../../shared/fonts.js";

export type CockpitUpdate = (patch: GalleryOptions) => void | Promise<void>;

export interface CockpitNavigationSource {
  focus(): number;
  subscribe(listener: () => void): () => void;
}

export function ControlScreen({
  state,
  update,
  application,
  initialTab = "scene",
  navigation,
}: {
  state: CockpitOptions;
  update: CockpitUpdate;
  application?: GallerySceneContext["application"];
  initialTab?: "scene" | "gui" | undefined;
  navigation?: CockpitNavigationSource | undefined;
}) {
  const kit = useGuiKit();
  const [tab, setTab] = useState<string>(initialTab);
  return (
    <Column gap={kit.spacing.section}>
      <SegmentedControl
        id="cockpit-control-tab"
        value={tab}
        onChange={setTab}
        options={[
          { value: "scene", label: "Scene controls" },
          { value: "gui", label: "GUI controls" },
        ]}
      />
      {tab === "scene" ? (
        <SceneControls state={state} update={update} />
      ) : (
        <GuiControls
          state={state}
          update={update}
          application={application}
          navigation={navigation}
        />
      )}
    </Column>
  );
}

function SceneControls({
  state,
  update,
}: {
  state: CockpitOptions;
  update: CockpitUpdate;
}) {
  const kit = useGuiKit();
  return (
    <Column gap={kit.spacing.section}>
      <Row gap={kit.spacing.section * 1.5} alignItems="start" minHeight={0}>
        <FormSection
          flex={1}
          title="Lighting"
          description="Set the mood of the cabin."
        >
          <LabelledSlider
            id="cockpit-illumination"
            label="Cabin illumination"
            value={state.illumination}
            min={0}
            max={1.5}
            step={0.05}
            bounds={false}
            onChange={(illumination) => update({ illumination })}
          />
          <Switch
            id="cockpit-lights-playing"
            checked={state.lightsPlaying}
            onCheckedChange={(lightsPlaying) => update({ lightsPlaying })}
          >
            Animate lights
          </Switch>
          <LabelledSlider
            id="cockpit-atmosphere"
            label="Atmosphere"
            value={state.atmosphere}
            min={0}
            max={1.5}
            step={0.05}
            bounds={false}
            onChange={(atmosphere) => update({ atmosphere })}
          />
          <Switch
            id="cockpit-atmosphere-playing"
            checked={state.atmospherePlaying}
            onCheckedChange={(atmospherePlaying) =>
              update({ atmospherePlaying })
            }
          >
            Exterior motion
          </Switch>
        </FormSection>
        <FormSection
          flex={1}
          title="Sensors"
          description="Tune the integrated dashboard instruments."
        >
          <LabelledSlider
            id="cockpit-gain"
            label="Signal gain"
            value={state.gain}
            min={0}
            max={1}
            step={0.05}
            bounds={false}
            onChange={(gain) => update({ gain })}
          />
          <Switch
            id="cockpit-scan"
            checked={state.scanning}
            onCheckedChange={(scanning) => update({ scanning })}
          >
            Continuous scan
          </Switch>
          <NumericStepper
            id="cockpit-route-range"
            label="Scanner range"
            units="km"
            value={state.range}
            min={1.2}
            max={4}
            step={0.1}
            precision={1}
            onChange={(range) => update({ range })}
          />
        </FormSection>
      </Row>
      <Row gap={kit.spacing.section * 1.5} alignItems="start" minHeight={0}>
        <FormSection flex={1} title="Flight mode">
          <RadioGroup
            id="cockpit-flight-mode"
            value={state.flightMode}
            options={[
              { value: "cruise", label: "Cruise" },
              { value: "approach", label: "Station approach" },
              { value: "survey", label: "Survey orbit" },
            ]}
            onChange={(flightMode) => update({ flightMode })}
          />
          <Checkbox
            id="cockpit-autopilot"
            checked={state.autopilot}
            onCheckedChange={(autopilot) => update({ autopilot })}
          >
            Autopilot engaged
          </Checkbox>
        </FormSection>
        <FormSection
          flex={1}
          title="Pulse transmitter"
          description="Charge the capacitor, then release a pulse."
        >
          <LabelledSlider
            id="cockpit-charge"
            label="Pulse charge"
            value={state.charge}
            min={0}
            max={1}
            step={0.1}
            bounds={false}
            format={(value) => `${Math.round(value * 100)}%`}
            onChange={(charge) => update({ charge })}
          />
          <Toolbar>
            <Button
              id="cockpit-charge-full"
              label="Charge pulse"
              variant="secondary"
              flex={1}
              disabled={state.charge === 1}
              onPress={() => update({ charge: 1 })}
            />
            <Button
              id="cockpit-pulse"
              label="Release pulse"
              flex={1}
              disabled={state.charge === 0}
              onPress={() => update({ pulse: state.pulse + 1, charge: 0 })}
            />
          </Toolbar>
          <TextLine
            text={`${state.pulse} pulses sent · ${state.scanning ? "scanner active" : "scanner paused"}`}
            tone="neutral"
            size="small"
          />
        </FormSection>
      </Row>
      <Separator />
      <Toolbar>
        <TextLine
          text="Changes apply immediately"
          size="small"
          tone="neutral"
          layout={{ flex: 1 }}
        />
        <Button
          id="cockpit-scene-reset"
          label="Reset scene"
          variant="secondary"
          onPress={() =>
            update({
              illumination: 1,
              lightsPlaying: true,
              atmosphere: 1,
              atmospherePlaying: true,
              gain: 0.64,
              range: 3.2,
              scanning: true,
              charge: 0.7,
              flightMode: "cruise",
              autopilot: true,
            })
          }
        />
      </Toolbar>
    </Column>
  );
}

const noSubscription = () => () => {};
const defaultSettings = {
  skin: "glass" as const,
  font: DEFAULT_DEMO_FONT,
  renderScale: 1,
};
function GuiControls({
  state,
  update,
  application,
  navigation,
}: {
  state: CockpitOptions;
  update: CockpitUpdate;
  application?: GallerySceneContext["application"];
  navigation?: CockpitNavigationSource | undefined;
}) {
  const kit = useGuiKit();
  const settings = useSyncExternalStore(
    application?.subscribe ?? noSubscription,
    application?.snapshot ?? (() => defaultSettings),
  );
  return (
    <Column gap={kit.spacing.section}>
      <Row gap={kit.spacing.section * 1.5} alignItems="start" minHeight={0}>
        <Form flex={1} gap={kit.spacing.section}>
          <FormSection
            title="Appearance"
            description="Choose a finish and tune its transparency."
          >
            <Field label="Interface skin">
              <Dropdown
                id="cockpit-skin"
                label="Interface skin"
                value={application ? settings.skin : state.skin}
                options={[
                  { key: "glass", label: "Liquid glass" },
                  { key: "cybertron", label: "Cybertron" },
                ]}
                onChange={(skin) =>
                  application
                    ? application.update({
                        skin: skin as "glass" | "cybertron",
                      })
                    : update({ skin })
                }
              />
            </Field>
            <Field label="Interface font">
              <Dropdown
                id="cockpit-font"
                label="Interface font"
                value={application ? settings.font : state.font}
                options={DEMO_FONTS}
                onChange={(value) => {
                  const font = demoFont(value).key;
                  return application
                    ? application.update({ font })
                    : update({ font });
                }}
              />
            </Field>
            <LabelledSlider
              id="cockpit-opacity"
              label="Panel opacity"
              value={state.panelOpacity}
              min={0.55}
              max={1}
              step={0.01}
              bounds={false}
              format={(value) => `${Math.round(value * 100)}%`}
              onChange={(panelOpacity) => update({ panelOpacity })}
            />
            <Field
              label="Render resolution"
              description="Changes sharpness without changing UI size."
            >
              <SegmentedControl
                id="cockpit-render-scale"
                value={String(settings.renderScale)}
                options={COCKPIT_RENDER_SCALES.map((scale) => ({
                  value: String(scale),
                  label: `${scale * 100}%`,
                }))}
                onChange={(value) =>
                  application?.update({ renderScale: Number(value) })
                }
              />
            </Field>
          </FormSection>
        </Form>
        <Form flex={1} gap={kit.spacing.section}>
          <FormSection
            title="Placement"
            description="Choose how the screens surround you."
          >
            <Field label="Screen projection">
              <SegmentedControl
                id="cockpit-surface"
                value={state.surface}
                options={[
                  { value: "flat", label: "Flat" },
                  { value: "curved", label: "Curved" },
                ]}
                onChange={(surface) => update({ surface })}
              />
            </Field>
            <LabelledSlider
              id="cockpit-projection"
              label="Distance from camera"
              value={state.projection}
              min={COCKPIT_SCREEN_DISTANCE.min}
              max={COCKPIT_SCREEN_DISTANCE.max}
              step={0.05}
              units="m"
              bounds={false}
              onChange={(projection) => update({ projection })}
            />
            <Checkbox
              id="cockpit-exploded"
              label="Exploded view"
              checked={state.exploded}
              onCheckedChange={(exploded) => update({ exploded })}
            />
            <Field
              label="Screen navigation"
              description="Swipe empty space or scroll to move between screens."
            >
              <CarouselNavigation
                navigation={navigation}
                carousel={state.carousel}
                update={update}
              />
            </Field>
          </FormSection>
        </Form>
      </Row>
      <FormSection
        title="Surface caching"
        description="Compare refresh cost and texture resolution. Active controls stay live."
      >
        <Row gap={kit.spacing.section} alignItems="start">
          <Form flex={1}>
            <Field label="Cache mode">
              <SegmentedControl
                id="cockpit-surface-caching"
                value={state.surfaceCaching}
                options={[
                  { value: "off", label: "Off" },
                  { value: "auto", label: "Auto" },
                  { value: "force", label: "Force" },
                ]}
                onChange={(surfaceCaching) => update({ surfaceCaching })}
              />
            </Field>
          </Form>
          <Form flex={1}>
            <LabelledSlider
              id="cockpit-surface-refresh"
              label="Refresh limit"
              value={state.surfaceRefreshHz}
              min={0.5}
              max={30}
              step={0.5}
              units="Hz"
              bounds={false}
              onChange={(surfaceRefreshHz) => update({ surfaceRefreshHz })}
            />
          </Form>
          <Form flex={1}>
            <Field label="Texture size cap">
              <Dropdown
                id="cockpit-surface-texture-size"
                label="Texture size cap"
                value={String(state.surfaceTextureSize)}
                options={[256, 512, 1024, 2048].map((size) => ({
                  key: String(size),
                  label: `${size} px`,
                }))}
                onChange={(value) =>
                  update({ surfaceTextureSize: Number(value) })
                }
              />
            </Field>
          </Form>
        </Row>
      </FormSection>
      <Separator />
      <TextLine
        text="Controls · System monitor · Terminal"
        size="small"
        tone="neutral"
      />
    </Column>
  );
}

function CarouselNavigation({
  navigation,
  carousel,
  update,
}: {
  navigation?: CockpitNavigationSource | undefined;
  carousel: number;
  update: CockpitUpdate;
}) {
  // Tracking intent only rerenders these buttons, without reauthoring the
  // surrounding controls or any of the attached Canvas Worlds.
  const focus = useSyncExternalStore(
    navigation?.subscribe ?? noSubscription,
    navigation?.focus ?? (() => carousel),
  );
  return (
    <Toolbar>
      <Button
        id="cockpit-previous"
        label="Previous"
        variant="secondary"
        flex={1}
        disabled={focus <= 0}
        onPress={() =>
          update({
            carousel: Math.max(
              0,
              Math.round(navigation?.focus() ?? carousel) - 1,
            ),
            carouselDragging: false,
          })
        }
      />
      <Button
        id="cockpit-next"
        label="Next"
        variant="secondary"
        flex={1}
        disabled={focus >= COCKPIT_PANE_COUNT - 1}
        onPress={() =>
          update({
            carousel: Math.min(
              COCKPIT_PANE_COUNT - 1,
              Math.round(navigation?.focus() ?? carousel) + 1,
            ),
            carouselDragging: false,
          })
        }
      />
    </Toolbar>
  );
}
