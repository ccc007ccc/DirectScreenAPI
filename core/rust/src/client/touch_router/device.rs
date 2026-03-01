use std::io;

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AbsInfo, AbsoluteAxisType, AttributeSet, Device, Key, PropType, UinputAbsSetup};

use super::TouchAxisCaps;

pub(super) fn read_touch_axis_caps(device: &Device) -> io::Result<TouchAxisCaps> {
    let Some(supported_abs) = device.supported_absolute_axes() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device_has_no_absolute_axes",
        ));
    };

    let required_axes = [
        AbsoluteAxisType::ABS_MT_POSITION_X,
        AbsoluteAxisType::ABS_MT_POSITION_Y,
        AbsoluteAxisType::ABS_MT_SLOT,
        AbsoluteAxisType::ABS_MT_TRACKING_ID,
    ];

    for axis in required_axes {
        if !supported_abs.contains(axis) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing_required_abs_axis:{}", axis.0),
            ));
        }
    }

    let abs_state = device.get_abs_state()?;
    let x = abs_state[AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize];
    let y = abs_state[AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize];
    let slot = abs_state[AbsoluteAxisType::ABS_MT_SLOT.0 as usize];
    let tracking = abs_state[AbsoluteAxisType::ABS_MT_TRACKING_ID.0 as usize];

    if x.maximum <= x.minimum || y.maximum <= y.minimum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid_abs_xy_range",
        ));
    }

    Ok(TouchAxisCaps {
        x_min: x.minimum,
        x_max: x.maximum,
        y_min: y.minimum,
        y_max: y.maximum,
        slot_min: slot.minimum,
        slot_max: slot.maximum,
        tracking_min: tracking.minimum,
        tracking_max: tracking.maximum,
    })
}

pub(super) fn build_virtual_touch_device(device: &Device) -> io::Result<VirtualDevice> {
    let mut key_set: AttributeSet<Key> = device
        .supported_keys()
        .map(|keys| keys.iter().collect())
        .unwrap_or_default();
    key_set.insert(Key::BTN_TOUCH);

    let mut props: AttributeSet<PropType> = device.properties().iter().collect();
    props.insert(PropType::DIRECT);

    let Some(abs_axes) = device.supported_absolute_axes() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device_has_no_absolute_axes",
        ));
    };

    let abs_state = device.get_abs_state()?;
    let device_name = format!(
        "DirectScreenAPI Touch Clone ({})",
        device.name().unwrap_or("unknown")
    );

    let mut builder = VirtualDeviceBuilder::new()?
        .name(&device_name)
        .input_id(device.input_id())
        .with_properties(&props)?
        .with_keys(&key_set)?;

    for axis in abs_axes.iter() {
        let info = abs_state[axis.0 as usize];
        let setup = UinputAbsSetup::new(
            axis,
            AbsInfo::new(
                info.value,
                info.minimum,
                info.maximum,
                info.fuzz,
                info.flat,
                info.resolution,
            ),
        );
        builder = builder.with_absolute_axis(&setup)?;
    }

    builder.build()
}
