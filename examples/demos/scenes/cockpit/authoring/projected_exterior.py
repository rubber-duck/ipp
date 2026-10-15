"""Separate exterior radiance into planet cities, atmosphere and stars."""

import runpy
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent


def expanded(values, radius):
    padded = np.pad(values, radius, mode="constant")
    result = np.zeros_like(values)
    height, width = values.shape
    for y in range(radius * 2 + 1):
        for x in range(radius * 2 + 1):
            np.maximum(result, padded[y : y + height, x : x + width], out=result)
    return result


def local_mean(values, radius):
    padded = np.pad(values, radius, mode="edge")
    integral = np.pad(padded.cumsum(axis=0).cumsum(axis=1), ((1, 0), (1, 0)))
    size = radius * 2 + 1
    return (
        integral[size:, size:]
        - integral[:-size, size:]
        - integral[size:, :-size]
        + integral[:-size, :-size]
    ) / size**2


def build(api, make_material):
    light = runpy.run_path(str(ROOT / "projected_lights.py"))
    load, save, linear = light["load"], light["save"], light["linear"]
    width, height = light["WIDTH"], light["HEIGHT"]
    _, mask = load("cockpit-exterior-emission-mask.png")
    _, original = load("cockpit-exterior.png")
    base_image, base = load("cockpit-exterior-unlit.png")
    base_image.pack()
    base_image.filepath = "//cockpit-exterior-unlit.png"
    original_linear = linear(original)
    delta = np.maximum(original_linear - linear(base), 0)

    # Green dominance identifies the limb despite the generated white city cores.
    # Fit a smooth curve so raster noise cannot deform the category boundary.
    green = np.maximum(
        mask[:, :, 1] - np.maximum(mask[:, :, 0], mask[:, :, 2]) * 0.8, 0
    )
    row = np.arange(height)
    ridge = np.argmax(green, axis=1)
    curve = np.polyval(np.polyfit(row, ridge, 4), row)
    # The atmospheric layer retains its original blue color and broad halo.
    # The remaining positive radiance is classified by the two sides of the limb;
    # this preserves faint stars omitted by the generated mask's bright cores.
    distance = np.arange(width)[None, :] - curve[:, None]
    atmosphere = np.exp(-((distance / 32) ** 2) / 2)
    cities = (1 - atmosphere) * np.clip((-distance - 35) / 20, 0, 1)
    stars = (1 - atmosphere) * np.clip((distance - 40) / 25, 0, 1)
    # Keep incidental image-generation changes to diffuse land/cloud/nebula
    # exposure out of animated layers. City support follows the generated network.
    city_seed = np.maximum(mask[:, :, 0] - mask[:, :, 1], 0)
    city_support = np.clip((expanded(city_seed, 3) - 0.035) * 8, 0, 1)
    city_support *= np.clip((delta[:, :, 0] - delta[:, :, 2] * 0.95) * 100, 0, 1)
    # The generated mask contains the bright stars. Recover omitted faint points
    # only where positive radiance has compact local contrast, never broad patches.
    luminance = delta @ np.array([0.2126, 0.7152, 0.0722])
    mean = local_mean(luminance, 3)
    highpass = np.maximum(luminance - mean, 0)
    contrast = highpass / np.maximum(mean, 0.002)
    faint_points = np.clip((highpass - 0.0005) * 500, 0, 1) * np.clip(
        (contrast - 0.25) * 2, 0, 1
    )
    star_support = np.maximum(
        np.clip(
            (expanded(np.maximum(mask[:, :, 2] - mask[:, :, 1], 0), 3) - 0.035) * 8,
            0,
            1,
        ),
        faint_points,
    )
    cities *= city_support
    stars *= star_support
    names = ("cities", "atmosphere", "stars")
    images = [
        save("cockpit-exterior-" + name + "-emission.png", delta * weight[:, :, None])
        for name, weight in zip(names, (cities, atmosphere, stars))
    ]
    material = make_material(base_image)
    material.name = "Exterior unlit with animated radiance"
    light["compose_preview"](material, images, ("Cities", "Atmosphere", "Stars"))
    full = [(0, 0), (width, 0), (width, height), (0, height)]
    sky = api("cockpit-space", full, 18, material, step=160)
    sky["exterior_body"] = True
    samples = list(range(0, height, 80)) + [height - 1]
    outline = (
        [(0, 0)]
        + [(min(width, float(curve[y]) + 100), y) for y in samples]
        + [(0, height)]
    )
    planet = api("cockpit-planet", outline, 17, material, step=100)
    planet["exterior_body"] = True
    records = []
    for name, image in zip(names, images):
        carrier_material = make_material(image)
        carrier = api("cockpit-exterior-" + name, full, 17, carrier_material, step=500)
        carrier.hide_render = True
        carrier["export_texture_carrier"] = True
        carrier["exterior_layer"] = name
        records.append({"id": carrier.name, "layer": name, "texture": image.name})
    return {
        "layers": records,
        "limb_fit_max_error_pixels": float(np.max(np.abs(curve - ridge))),
        "encoding": "sRGB-encoded positive linear RGB; original colors retained",
    }
