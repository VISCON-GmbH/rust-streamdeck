use std::{collections::HashSet, vec};

use crate::Kind;

/// The InputEvent enum represents the different types of input events that can be generated by Streamdeck devices.
/// Most streamdeck devices only have buttons, the Streamdeck Plus also has dials and a touchscreen.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum InputEvent {
    Button {
        index: u8,
        released: bool,
    },
    Dial {
        index: u8,
        ///Press will have a value if the dial was pressed or released, None if the dial was turned
        press: Option<bool>,
        ///Delta will have a value if the dial was turned, None if the dial was pressed or released
        delta: Option<i8>,
    },
    Touch {
        x: u16,
        y: u16,
        touch_type: TouchType,
    },
}

///Different types of touch events that can be generated by streamdecks with touchscreens
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TouchType {
    ///A short touch event
    Short,
    ///A long touch event
    Long,
    ///A drag event, x and y are the end coordinates of the drag
    Drag { 
        x: u16,
        y: u16,
    },
}

pub(crate) struct TouchDataIndices {
    pub event_type_index: usize,
    pub x_low: usize,
    pub x_high: usize,
    pub y: usize,
    pub drag_x_low: usize,
    pub drag_x_high: usize,
    pub drag_y: usize,

}

#[derive(Debug, Clone)]
pub(crate) struct InputManager {
    pressed_keys: HashSet<u8>,
    pressed_dials: HashSet<usize>,
}

impl InputManager {
    pub(crate) fn new() -> Self {
        InputManager {
            pressed_keys: HashSet::new(),
            pressed_dials: HashSet::new(),
        }
    }


    pub(crate) fn handle_input(
        &mut self,
        cmd: &[u8; 36],
        kind: &crate::Kind,
    ) -> Result<Vec<InputEvent>, crate::Error> {
        //SD Plus has Dials and Touchscreen, other models only have buttons
        if kind == &Kind::Plus {
            return Ok(match cmd[1] {
                0 => self.handle_button_event(&cmd, &kind),
                2 => self.handle_touchscreen_event(&cmd, &kind)?,
                3 => self.handle_dial_event(&cmd, &kind),
                _ => return Err(crate::Error::InvalidInputTypeIndex),
            });
        }
        Ok(self.handle_button_event(&cmd, &kind))
    }

    fn handle_touchscreen_event(&self, cmd: &[u8; 36], kind: &crate::Kind) -> Result<Vec<InputEvent>, crate::Error> {
        let indices = kind.touch_data_indices();

        if indices.is_none() {
            return Err(crate::Error::InvalidTouchscreenSegmentIndex);
        }
        let indices = indices.unwrap();
        /*
         * Indices are hardcoded for now, as the SD+ is the only one with a touchscreen.
         * TODO: create a new fn in Kind struct to return the relevant indices for the current device
         * if more Streamdeck models with touchscreens are released. 
        */
        let touch_type = match cmd[indices.event_type_index] {
            1 => TouchType::Short,
            2 => TouchType::Long,
            3 => TouchType::Drag{ 
                x: ((cmd[indices.drag_x_high] as u16) << 8) + cmd[indices.drag_x_low] as u16,
                y: cmd[indices.drag_y] as u16,
            },
            _ => return Err(crate::Error::InvalidTouchType)
        };

        Ok(vec![InputEvent::Touch {
            touch_type,
            x: ((cmd[indices.x_high] as u16) << 8) + cmd[indices.x_low] as u16,
            y: cmd[indices.y] as u16,
        }])
    }

    fn handle_dial_event(&mut self, cmd: &[u8; 36], kind: &crate::Kind) -> Vec<InputEvent> {
        let offset = kind.dial_data_offset();
        let dials = kind.dials() as usize;
        let press = cmd[kind.dial_press_flag_index()] == 0;
        let mut events = Vec::new();

        if !press {
            for i in offset..offset + dials {
                if cmd[i] == 0 {
                    continue;
                }
                let delta: i8;
                if cmd[(i) as usize] > 127 {
                    //convert to signed u8 and invert. subtract 1 to make it 0-based
                    delta = -((255 - cmd[(i) as usize]) as i8) -1;
                } 
                else {
                    delta = cmd[(i) as usize] as i8;
                }
                events.push(InputEvent::Dial {
                    index: (i - offset) as u8,
                    press: None,
                    delta: Some(delta),
                });
            }
            return events;
        }

        let mut fresh_presses = HashSet::new();
        for i in offset..offset + dials {
            if cmd[i] == 1 {
                let dial = i - offset;
                if self.pressed_dials.contains(&dial) {
                    continue;
                }
                fresh_presses.insert(dial);
                events.push(InputEvent::Dial {
                    index: dial as u8,
                    press: Some(press),
                    delta: None,
                });
            }
        }

        self.pressed_dials.retain(|dial| {
            if cmd[(offset + *dial) as usize] == 0 && !fresh_presses.contains(dial) {
                events.push(InputEvent::Dial {
                    index: *dial as u8,
                    press: Some(!press),
                    delta: None,
                });
                return false;
            }
            true
        });

        self.pressed_dials.extend(fresh_presses);
        events
    }


    fn handle_button_event(&mut self, cmd: &[u8; 36], kind: &crate::Kind) -> Vec<InputEvent> {
        let mut fresh_presses = HashSet::new();
        let mut events = Vec::new();
        let keys = kind.keys() as usize;
        let offset = kind.key_data_offset() + 1;
        
        for i in offset..offset + keys  {

            if cmd[i] == 0 {
                continue;
            }
            let button = (i - offset) as u8;
            // If the button was already reported as pressed, skip it
            if self.pressed_keys.contains(&button) {
                continue;
            }
            // If the button press is fresh, add it to the fresh_presses HashSet and the events Vec
            fresh_presses.insert(button);
            events.push(InputEvent::Button {
                index: button,
                released: false,
            });
        }

        // Remove released buttons from the pressed_keys HashSet and add them to the events Vec as released
        self.pressed_keys.retain(|button| {
            if cmd[offset + *button as usize] == 0 && !fresh_presses.contains(button) {
                events.push(InputEvent::Button {
                    index: *button,
                    released: true,
                });
                return false;
            }
            true
        });

        // Add the fresh_presses HashSet to the pressed_keys HashSet
        self.pressed_keys.extend(fresh_presses);
        events
    }
}
