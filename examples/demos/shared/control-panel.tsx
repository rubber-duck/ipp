import {
  Button,
  Dropdown,
  LabelledSlider,
  Field,
  Toolbar,
} from "@ipp/react/gui-kit";
import type { GallerySceneControlsProps } from "./scene.js";

export function sceneActions({ mount, report }: GallerySceneControlsProps) {
  const run = (work: Promise<unknown>) => {
    return work.then(() => {}, report);
  };
  return {
    action: (name: string, args?: unknown) => run(mount.action(name, args)),
    update: (patch: Readonly<Record<string, unknown>>) =>
      run(mount.update(patch)),
  };
}

export function SceneChoice({
  id,
  label,
  value,
  choices,
  change,
}: {
  id: string;
  label: string;
  value: string;
  choices: readonly { key: string; label: string }[];
  change(value: string): void | Promise<void>;
}) {
  return (
    <Field id={`${id}/field`} label={label}>
      <Dropdown
        id={id}
        label={label}
        value={value}
        options={choices}
        onChange={change}
      />
    </Field>
  );
}

export function SceneSlider({
  id,
  label,
  value,
  min,
  max,
  step = 0.05,
  change,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  change(value: number): void | Promise<void>;
}) {
  return (
    <LabelledSlider
      id={id}
      label={label}
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={change}
      bounds={false}
      width={-1}
    />
  );
}

export function SceneButtons({
  buttons,
}: {
  buttons: readonly { id: string; label: string; press(): void }[];
}) {
  return (
    <Toolbar>
      {buttons.map((button) => (
        <Button
          key={button.id}
          id={button.id}
          label={button.label}
          onPress={button.press}
          variant="secondary"
          width={0}
          flex={1}
        />
      ))}
    </Toolbar>
  );
}
