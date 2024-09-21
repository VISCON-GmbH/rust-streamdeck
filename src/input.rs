use std::{collections::HashSet, time::Duration, vec};

use crate::{KeyDirection, Kind, StreamDeck};



/// The InputEvent enum represents the different types of input events that can be generated by Streamdeck devices.
/// Most streamdeck devices only have buttons, the Streamdeck Plus also has dials and a touchscreen.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum InputEvent {
    ///Button event, index is the zero-based index of the button, released is true if the button was released
    Button {
        index: u8,
        action: ButtonAction,
    },
    ///Dial event, index is the zero-based index of the dial, action is the performed DialAction
    Dial {
        index: u8,
        action: DialAction
    },
    ///Touch event, x and y are the coordinates of the touch event on the touchscreen, action is the performed TouchAction
    Touch {
        x: u16,
        y: u16,
        action: TouchAction,
    },
}

///Different types of button events that can be generated by streamdeck devices
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ButtonAction {
    Pressed,
    Released,
}

///Different types of touch events that can be generated by streamdecks with touchscreens
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TouchAction {
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

///Different types of dial events that can be generated by streamdecks with dials
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DialAction {
    ///The dial was pressed
    Pressed,
    ///The dial was released
    Released,
    ///The dial was turned, the value is the delta of the turn. 
    ///Negative values are counter-clockwise, positive values are clockwise
    Turned(i8),
}

///Manages inputs for the Streamdeck device. Keeps track of pressed keys and dials and touchscreens and generates InputEvents
pub struct InputManager<'a> {
    deck: &'a mut StreamDeck,
    pressed_keys: HashSet<u8>,
    pressed_dials: HashSet<usize>,
}

impl <'a> InputManager<'a> {
    pub fn new(deck: &'a mut StreamDeck) -> Self {
        InputManager {
            deck,
            pressed_keys: HashSet::new(),
            pressed_dials: HashSet::new(),
        }
    }

    ///Handles input events for the Streamdeck device and returns a Vec of InputEvents
    pub fn handle_input(
        &mut self,
        timeout: Option<Duration>
    ) -> Result<Vec<InputEvent>, crate::Error> {
        let cmd = self.deck.read_input(timeout)?;
        let kind = self.deck.kind;
        //SD Plus has Dials and Touchscreen, other models only have buttons
        if kind == Kind::Plus {
            return Ok(match cmd[1] {
                0 => self.handle_button_event(&cmd, &kind),
                2 => self.handle_touchscreen_event(&cmd, &kind)?,
                3 => self.handle_dial_event(&cmd, &kind),
                _ => return Err(crate::Error::UnsupportedInput),
            });
        }
        Ok(self.handle_button_event(&cmd, &kind))
    }

    ///Handles touchscreen events (short touch, long touch, drag) and returns a Vec of InputEvents
    fn handle_touchscreen_event(&self, cmd: &[u8; 36], kind: &Kind) -> Result<Vec<InputEvent>, crate::Error> {
        let indices = kind.touch_data_indices();

        if indices.is_none() {
            return Err(crate::Error::UnsupportedInput);
        }
        let indices = indices.unwrap();
        /*
         * Indices are hardcoded for now, as the SD+ is the only one with a touchscreen.
         * TODO: create a new fn in Kind struct to return the relevant indices for the current device
         * if more Streamdeck models with touchscreens are released. 
        */
        let action = match cmd[indices.event_type_index] {
            1 => TouchAction::Short,
            2 => TouchAction::Long,
            3 => TouchAction::Drag{ 
                x: ((cmd[indices.drag_x_high] as u16) << 8) + cmd[indices.drag_x_low] as u16,
                y: cmd[indices.drag_y] as u16,
            },
            _ => return Err(crate::Error::UnsupportedInput)
        };

        Ok(vec![InputEvent::Touch {
            action,
            x: ((cmd[indices.x_high] as u16) << 8) + cmd[indices.x_low] as u16,
            y: ((cmd[indices.y_high] as u16) << 8) + cmd[indices.y_low] as u16,
        }])
    }

    ///Handles dial events (press, release, turn) and returns a Vec of InputEvents
    fn handle_dial_event(&mut self, cmd: &[u8; 36], kind: &Kind) -> Vec<InputEvent> {
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
                    //convert to i8 and invert. subtract 1 to make it 0-based
                    delta = -((255 - cmd[(i) as usize]) as i8) -1;
                } 
                else {
                    delta = cmd[(i) as usize] as i8;
                }
                events.push(InputEvent::Dial {
                    index: (i - offset) as u8,
                    action: DialAction::Turned(delta),
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
                    action: DialAction::Pressed,
                });
            }
        }

        self.pressed_dials.retain(|dial| {
            if cmd[(offset + *dial) as usize] == 0 && !fresh_presses.contains(dial) {
                events.push(InputEvent::Dial {
                    index: *dial as u8,
                    action: DialAction::Released,
                });
                return false;
            }
            true
        });

        self.pressed_dials.extend(fresh_presses);
        events
    }

    ///Handles button events (press, release) and returns a Vec of InputEvents
    fn handle_button_event(&mut self, cmd: &[u8; 36], kind: &Kind) -> Vec<InputEvent> {
        let mut fresh_presses = HashSet::new();
        let mut events = Vec::new();
        let keys = kind.keys() as usize;
        let offset = kind.key_data_offset();
        
        for i in offset..offset + keys  {

            if cmd[i] == 0 {
                continue;
            }

            let button = match self.deck.kind.key_direction() {
                KeyDirection::RightToLeft => keys as u8 - (i - offset) as u8,
                KeyDirection::LeftToRight => i as u8 + self.deck.kind.key_index_offset(),
            };

            // If the button was already reported as pressed, skip it
            if self.pressed_keys.contains(&button) {
                continue;
            }


            // If the button press is fresh, add it to the fresh_presses HashSet and the events Vec
            fresh_presses.insert(button);
            events.push(InputEvent::Button {
                index: button,
                action: ButtonAction::Pressed,
            });
        }

        // Remove released buttons from the pressed_keys HashSet and add them to the events Vec as released
        self.pressed_keys.retain(|button| {
            if cmd[offset + *button as usize] == 0 && !fresh_presses.contains(button) {
                events.push(InputEvent::Button {
                    index: *button,
                    action: ButtonAction::Released,
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