import { useState } from "react";
import { Asset, Resources, assetRef } from "@ipp/react";
import {
  Accordion,
  Avatar,
  Button,
  Calendar,
  Carousel,
  Collapsible,
  Column,
  DateInput,
  DatePicker,
  Drawer,
  ImageCropper,
  Meter,
  Pagination,
  Rating,
  Resizable,
  TextField,
  TextLine,
  Timeline,
  TimeInput,
  Toggle,
  ToggleGroup,
  Waterfall,
  useGuiKit,
} from "@ipp/react/gui-kit";

export type GallerySection =
  | "selection"
  | "calendar"
  | "editing"
  | "picker"
  | "layout"
  | "feed"
  | "assets"
  | "drawer";
type Changed = (value: string) => void;

function bitmap(orange: boolean) {
  const bytes = new Uint8Array(16 + 16 * 16 * 4);
  bytes.set([73, 80, 80, 84]);
  const header = new DataView(bytes.buffer);
  for (const [index, value] of [3, 16, 16].entries())
    header.setUint32(4 + index * 4, value, true);
  for (let y = 0; y < 16; y++)
    for (let x = 0; x < 16; x++) {
      bytes.set(
        x < 8
          ? orange
            ? [252, 110, 27, 255]
            : [130, 70, 180, 255]
          : [32, 180, 60, 255],
        16 + (y * 16 + x) * 4,
      );
    }
  return bytes;
}
const photo = assetRef("gallery-photo");

function GalleryContents({
  section,
  change,
}: {
  section: GallerySection;
  change: Changed;
}) {
  const [loaded, setLoaded] = useState(false);
  const [date, setDate] = useState<string | null>("2024-02-28");
  const [singleSlide, setSingleSlide] = useState(false);
  const [measure, setMeasure] = useState(25);
  const kit = useGuiKit();
  switch (section) {
    case "selection":
      return (
        <>
          <Toggle
            id="pin"
            label="Pin"
            onPressedChange={(v) => change("pin/" + v)}
          />
          <ToggleGroup
            id="tools"
            label="Tools"
            multiple
            options={[
              { key: "slow", label: "Slow" },
              { key: "fast", label: "Fast" },
            ]}
            onChange={(v) => change("tools/" + v.join(","))}
          />
          <Pagination
            id="pages"
            totalItems={240}
            pageSize={20}
            onPageChange={(v) => change("page/" + v)}
          />
          <Meter
            id="load"
            label="Load"
            min={0}
            max={100}
            low={15}
            high={70}
            optimum={40}
            value={measure}
            width={190}
          />
          <Button id="sample" label="Sample" onPress={() => setMeasure(120)} />
          <Rating
            id="score"
            label="Score"
            precision={0.5}
            onChange={(v) => change("score/" + v)}
          />
          <TextLine
            id="motion-preference"
            text={kit.reducedMotion ? "Reduced" : "Full"}
          />
        </>
      );
    case "calendar":
      return (
        <Calendar
          id="month"
          mode="range"
          min="2024-02-20"
          max="2024-03-10"
          defaultMonth="2024-02-01"
          isDateDisabled={(date) => date === "2024-02-28"}
          onChange={(v) =>
            change(v ? v.start + "/" + (v.end ?? "pending") : "clear")
          }
        />
      );
    case "editing":
      return (
        <>
          <DateInput
            id="date"
            value={date}
            onChange={(v) => {
              setDate(v);
              change("date/" + v);
            }}
          />
          <Button
            id="external-date"
            label="Change date"
            onPress={() =>
              setDate(date === "2024-02-28" ? "2024-03-01" : "2024-02-28")
            }
          />
          <TimeInput
            id="time"
            hour12
            defaultValue="00:15"
            onChange={(v) => change("time/" + v)}
          />
          <TimeInput
            id="seven"
            defaultValue="00:00:00"
            showSeconds
            stepSeconds={7}
            onChange={(v) => change("seven/" + v)}
          />
        </>
      );
    case "picker":
      return (
        <DatePicker
          id="pick"
          defaultValue="2024-02-28"
          onChange={(v) => change("pick/" + v)}
        />
      );
    case "layout":
      return (
        <>
          <Resizable
            id="split"
            width={300}
            height={40}
            panes={[
              {
                key: "left",
                min: 40,
                collapsible: true,
                content: <TextLine text="Files" />,
              },
              { key: "right", min: 40, content: <TextLine text="Preview" /> },
            ]}
            onSizesChange={(v) => change("resize/" + v.left)}
          />
          <Carousel
            id="slides"
            width={240}
            height={40}
            slides={[
              {
                key: "one",
                label: "Summary",
                content: <TextLine text="There are two slides" />,
              },
              {
                key: "two",
                label: "Details",
                content: (
                  <Button
                    id="detail"
                    label="Inspect"
                    onPress={() => change("inspect")}
                  />
                ),
              },
            ].slice(0, singleSlide ? 1 : 2)}
            onChange={(v) => change("slide/" + v)}
          />
          <Button
            id="remove-last"
            label="Remove last slide"
            onPress={() => setSingleSlide(true)}
          />
          <Collapsible
            id="more"
            label="Notes"
            defaultExpanded
            onExpandedChange={(v) => change("more/" + v)}
          >
            <TextField id="retained" label="Draft" defaultValue="Keep me" />
          </Collapsible>
          <Accordion
            id="sections"
            items={[
              { key: "a", label: "First", content: <TextLine text="One" /> },
              { key: "b", label: "Second", content: <TextLine text="Two" /> },
            ]}
            onExpandedChange={(v) => change("expanded/" + v.join(","))}
          />
        </>
      );
    case "feed":
      return (
        <>
          <Timeline
            id="history"
            items={[
              {
                key: "open",
                timestamp: "09:30",
                title: "Opened",
                body: "Work began.",
              },
              {
                key: "done",
                timestamp: "09:31",
                title: "Completed",
                actions: (
                  <Button
                    id="result"
                    label="View result"
                    onPress={() => change("result")}
                  />
                ),
              },
            ]}
          />
          <Waterfall
            id="trace"
            labelWidth={120}
            spans={[
              {
                key: "request",
                label: "Request",
                start: 0,
                duration: 12,
                children: [
                  { key: "db", label: "Database", start: 2, duration: 4 },
                ],
              },
            ]}
            onSelect={(v) => change("span/" + v)}
          />
        </>
      );
    case "assets":
      return (
        <>
          {loaded && (
            <Resources>
              <Asset id="gallery-photo" kind={2} data={true} encode={bitmap} />
            </Resources>
          )}
          <Avatar id="person" name="Rae Fox" source={photo} size={42} />
          <ImageCropper
            id="crop"
            source={photo}
            imageWidth={16}
            imageHeight={16}
            previewWidth={128}
            previewHeight={88}
            aspect={1}
            defaultCrop={{ x: 4, y: 4, width: 8, height: 8 }}
            onCropChange={(v) => change("crop/" + v.x + "/" + v.width)}
          />
          <Button
            id="photo-toggle"
            label="Change image"
            onPress={() => setLoaded((ready) => !ready)}
          />
        </>
      );
    default:
      return null;
  }
}
export function ComponentGallery({
  section,
  change = () => {},
}: {
  section: GallerySection;
  change?: Changed;
}) {
  const [open, setOpen] = useState(false);
  if (section === "drawer")
    return (
      <>
        <Column width={240}>
          <Button
            id="show-drawer"
            label="Show details"
            onPress={() => setOpen(true)}
          />
        </Column>
        <Drawer
          id="drawer"
          label="Details"
          edge="bottom"
          open={open}
          onOpenChange={setOpen}
          snapPoints={[80, 160]}
          onSnapChange={(v) => change("snap/" + v)}
        >
          <TextLine text="Drawer content" />
        </Drawer>
      </>
    );
  return (
    <Column
      key={section}
      id="gallery"
      width={352}
      gap={4}
      padding={4}
      alignX="start"
      alignY="start"
    >
      <GalleryContents section={section} change={change} />
    </Column>
  );
}
