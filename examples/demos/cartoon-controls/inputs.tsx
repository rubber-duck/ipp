import { Groups } from "./groups.js";
import { MarkerPreview } from "./marker-preview.js";
import { useState } from "react";
import {
  Autocomplete,
  Button,
  Checkbox,
  ColorPicker,
  Column,
  Dropdown,
  Field,
  Form,
  FormSection,
  Knob,
  LabelledSlider,
  MultiSelect,
  NumericStepper,
  OptionList,
  RadioGroup,
  RangeSlider,
  Row,
  SearchableDropdown,
  SecondaryButton,
  SegmentedControl,
  Switch,
  TextField,
  TextLine,
  parseHex,
} from "@ipp/react/gui-kit";

const routes = [
  "Meadow",
  "Pine forest",
  "Crystal cave",
  "Cloud bridge",
  "Stone tower",
  "Hidden garden",
].map((label) => ({ key: label, label }));
const modes = ["Easy", "Normal", "Hard"].map((label) => ({
  value: label.toLowerCase(),
  label,
}));

export interface Settings {
  difficulty: string;
  music: boolean;
}

export function GameSettings({
  saved,
  save,
}: {
  saved: Settings;
  save: (value: Settings) => void;
}) {
  const [difficulty, choose] = useState(saved.difficulty);
  const [music, sound] = useState(saved.music);
  const [message, report] = useState("Game settings");
  return (
    <Column gap="field">
      <TextLine text={message} size="display" />
      <Field label="Difficulty">
        <SegmentedControl
          id="difficulty"
          options={modes}
          value={difficulty}
          onChange={(value) => {
            choose(value);
            report("Game settings");
          }}
        />
      </Field>
      <Switch
        id="music"
        label="Play music"
        checked={music}
        onCheckedChange={(value) => {
          sound(value);
          report("Game settings");
        }}
      />
      <Row gap="label">
        <Button
          id="save-settings"
          label="Save settings"
          onPress={() => {
            save({ difficulty, music });
            report("Settings saved");
          }}
        />
        <Button
          id="defaults"
          variant="secondary"
          label="Defaults"
          onPress={() => {
            choose("normal");
            sound(true);
            report("Defaults restored");
          }}
        />
      </Row>
    </Column>
  );
}

export function Actions({ wide }: { wide: boolean }) {
  const [name, setName] = useState("Pip");
  const [count, setCount] = useState(0);
  return (
    <Form>
      <Groups
        wide={wide}
        left={
          <FormSection title="Player details">
            <TextField
              id="runner-name"
              label="Runner name"
              value={name}
              onChange={setName}
            />
            <TextField
              id="join-code"
              label="Private join code"
              masked
              defaultValue="forest"
            />
            <Checkbox id="markers" label="Show trail markers" defaultChecked />
            <Switch
              id="vibration"
              label="Controller vibration"
              defaultChecked
            />
          </FormSection>
        }
        right={
          <FormSection title="Trail gems">
            <Row gap="label">
              <Button
                id="collect"
                label="Collect"
                onPress={() => setCount((n) => n + 1)}
              />
              <SecondaryButton
                label="Undo"
                onPress={() => setCount((n) => Math.max(0, n - 1))}
                disabled={!count}
              />
              <Button
                variant="destructive"
                label="Clear"
                onPress={() => setCount(0)}
              />
            </Row>
            <TextLine text={`${name || "Runner"} · ${count} gems`} />
            <Button label="Locked" disabled fitWidth />
          </FormSection>
        }
      />
    </Form>
  );
}

export function Choices({ wide }: { wide: boolean }) {
  const [route, choose] = useState("Meadow");
  const [search, setSearch] = useState("");
  return (
    <Groups
      wide={wide}
      left={
        <>
          <Field label="Radio group">
            <RadioGroup id="radio-mode" options={modes} defaultValue="normal" />
          </Field>
          <Field label="Dropdown">
            <Dropdown
              id="route-pick"
              label="Route"
              options={routes}
              value={route}
              onChange={choose}
            />
          </Field>
          <Field label="Searchable dropdown">
            <SearchableDropdown
              id="search-route"
              options={routes}
              placeholder="Find a route"
              searchPlaceholder="Search routes"
            />
          </Field>
        </>
      }
      right={
        <>
          <Field label="Multiple selection">
            <MultiSelect
              id="gear-select"
              options={routes.slice(0, 4)}
              defaultValue={["Meadow", "Crystal cave"]}
              placeholder="Choose stages"
            />
          </Field>
          <Field label="Autocomplete">
            <Autocomplete
              id="route-auto"
              inputValue={search}
              onInputChange={setSearch}
              suggestions={routes.filter((r) =>
                r.label.toLowerCase().includes(search.toLowerCase()),
              )}
              onSelect={setSearch}
              placeholder="Type a stage name"
            />
          </Field>
          <Field label="Option list">
            <OptionList
              id="route-list"
              options={routes}
              selected={[route]}
              onPick={choose}
              maxRows={3}
            />
          </Field>
        </>
      }
    />
  );
}

export function Values({ wide }: { wide: boolean }) {
  const [volume, setVolume] = useState(65);
  return (
    <Groups
      wide={wide}
      left={
        <>
          <LabelledSlider
            id="volume"
            label="Volume"
            min={0}
            max={100}
            step={1}
            units="%"
            value={volume}
            onChange={setVolume}
            scale={{ count: 5 }}
          />
          <RangeSlider
            id="range"
            label="Altitude range"
            min={0}
            max={100}
            defaultValue={[20, 75]}
            units="m"
          />
          <NumericStepper
            id="lap-count"
            label="Laps"
            min={1}
            max={10}
            step={1}
            defaultValue={3}
          />
        </>
      }
      right={
        <Row gap="label" alignItems="start">
          <Knob
            id="camera-knob"
            label="Camera"
            min={0}
            max={100}
            defaultValue={45}
            units="%"
          />
          <LabelledSlider
            width={72}
            id="vertical-volume"
            label="Mix"
            vertical
            length={118}
            min={0}
            max={100}
            value={volume}
            onChange={setVolume}
          />
          <RangeSlider
            width={85}
            id="vertical-range"
            label="Limits"
            vertical
            length={118}
            min={0}
            max={100}
            defaultValue={[25, 70]}
          />
        </Row>
      }
    />
  );
}

const swatches = [
  ["Gold", "#FFCA45"],
  ["Sky", "#75DCFF"],
  ["Grass", "#80DB85"],
  ["Coral", "#FF8F79"],
].map(([label, hex]) => ({ label: label!, value: parseHex(hex!)! }));
export function Colours({ wide }: { wide: boolean }) {
  const [value, change] = useState(swatches[2]!.value);
  const Layout = wide ? Row : Column;
  return (
    <Layout gap="field" alignItems="start">
      <ColorPicker
        id="trail-color"
        label="Marker colour"
        value={value}
        onChange={change}
        alpha
        presets={swatches}
      />
      <MarkerPreview value={value} wide={wide} />
    </Layout>
  );
}
