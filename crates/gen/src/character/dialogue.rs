//! Branching dialogue system for NPCs.
//!
//! Implements Spec 1.4: `gen_set_npc_dialogue` — Branching Dialogue Trees

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single node in a dialogue tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueNode {
    /// Unique node identifier.
    pub id: String,
    /// The text spoken at this node.
    pub text: String,
    /// Optional speaker name override (defaults to NPC name).
    #[serde(default)]
    pub speaker: Option<String>,
    /// Available choices at this node.
    #[serde(default)]
    pub choices: Vec<DialogueChoice>,
}

/// A choice option in a dialogue node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueChoice {
    /// Choice text shown to player.
    pub text: String,
    /// ID of the next node, or None to end dialogue.
    #[serde(default)]
    pub next_node_id: Option<String>,
}

/// Component storing a dialogue tree on an NPC.
#[derive(Component, Clone, Default)]
pub struct DialogueTree {
    /// All dialogue nodes, keyed by ID.
    pub nodes: HashMap<String, DialogueNode>,
    /// Starting node ID.
    pub start_node: String,
    /// Trigger type: "proximity" or "click".
    pub trigger: DialogueTrigger,
    /// Trigger radius.
    pub trigger_radius: f32,
}

impl DialogueTree {
    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&DialogueNode> {
        self.nodes.get(id)
    }

    /// Get the starting node.
    pub fn get_start_node(&self) -> Option<&DialogueNode> {
        self.get_node(&self.start_node)
    }
}

/// How dialogue is triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DialogueTrigger {
    /// Player presses E within range.
    #[default]
    Click,
    /// Automatically when player enters range.
    Proximity,
}

/// Resource tracking active dialogue state.
#[derive(Resource, Clone, Default)]
pub struct DialogueState {
    /// The NPC entity currently in dialogue (if any).
    pub active_npc: Option<Entity>,
    /// Current dialogue node ID.
    pub current_node: Option<String>,
    /// Whether text is still typing out.
    pub is_typing: bool,
    /// Typewriter progress (0.0 - 1.0).
    pub typewriter_progress: f32,
    /// Cooldown for proximity triggers.
    pub proximity_cooldown: f32,
}

/// Parameters for setting NPC dialogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetDialogueParams {
    /// NPC entity name/ID.
    pub npc_id: String,
    /// Dialogue nodes.
    pub nodes: Vec<DialogueNodeDef>,
    /// Starting node ID.
    pub start_node: String,
    /// Trigger type: "proximity" or "click".
    #[serde(default)]
    pub trigger: String,
    /// Trigger radius (default: 3.0).
    #[serde(default = "default_trigger_radius")]
    pub trigger_radius: f32,
}

fn default_trigger_radius() -> f32 {
    3.0
}

/// Simplified dialogue node definition for MCP input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueNodeDef {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub choices: Vec<DialogueChoiceDef>,
}

/// Simplified choice definition for MCP input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueChoiceDef {
    pub text: String,
    #[serde(default)]
    pub next_node_id: Option<String>,
}

impl From<DialogueNodeDef> for DialogueNode {
    fn from(def: DialogueNodeDef) -> Self {
        Self {
            id: def.id,
            text: def.text,
            speaker: def.speaker,
            choices: def
                .choices
                .into_iter()
                .map(|c| DialogueChoice {
                    text: c.text,
                    next_node_id: c.next_node_id,
                })
                .collect(),
        }
    }
}

impl From<SetDialogueParams> for DialogueTree {
    fn from(params: SetDialogueParams) -> Self {
        let trigger = match params.trigger.to_lowercase().as_str() {
            "proximity" => DialogueTrigger::Proximity,
            _ => DialogueTrigger::Click,
        };

        let nodes: HashMap<String, DialogueNode> = params
            .nodes
            .into_iter()
            .map(|n| (n.id.clone(), n.into()))
            .collect();

        Self {
            nodes,
            start_node: params.start_node,
            trigger,
            trigger_radius: params.trigger_radius,
        }
    }
}

/// System to detect proximity dialogue triggers.
pub fn proximity_dialogue_trigger_system(
    time: Res<Time>,
    player_query: Query<&Transform, With<Player>>,
    npc_query: Query<(Entity, &Transform, &DialogueTree), With<Npc>>,
    mut dialogue_state: ResMut<DialogueState>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };

    // Skip if dialogue already active
    if dialogue_state.active_npc.is_some() {
        return;
    }

    // Update cooldown
    if dialogue_state.proximity_cooldown > 0.0 {
        dialogue_state.proximity_cooldown -= time.delta_secs();
        return;
    }

    // Check for proximity triggers
    for (entity, transform, dialogue) in npc_query.iter() {
        if dialogue.trigger != DialogueTrigger::Proximity {
            continue;
        }

        let distance = transform.translation.distance(player_transform.translation);
        if distance <= dialogue.trigger_radius {
            // Start dialogue
            dialogue_state.active_npc = Some(entity);
            dialogue_state.current_node = Some(dialogue.start_node.clone());
            dialogue_state.typewriter_progress = 0.0;
            dialogue_state.proximity_cooldown = 5.0; // 5 second cooldown
            break;
        }
    }
}

/// System to handle click/dialogue interaction prompt.
pub fn click_dialogue_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    player_query: Query<&Transform, With<Player>>,
    npc_query: Query<(Entity, &Transform, &DialogueTree), With<Npc>>,
    mut dialogue_state: ResMut<DialogueState>,
) {
    // Only respond to E key press
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }

    // Skip if dialogue already active
    if dialogue_state.active_npc.is_some() {
        return;
    }

    let Ok(player_transform) = player_query.single() else {
        return;
    };

    // Find closest NPC with dialogue in range
    let mut closest: Option<(Entity, f32, String)> = None;

    for (entity, transform, dialogue) in npc_query.iter() {
        if dialogue.trigger != DialogueTrigger::Click {
            continue;
        }

        let distance = transform.translation.distance(player_transform.translation);
        if distance <= dialogue.trigger_radius {
            match closest {
                None => closest = Some((entity, distance, dialogue.start_node.clone())),
                Some((_, best_dist, _)) if distance < best_dist => {
                    closest = Some((entity, distance, dialogue.start_node.clone()));
                }
                _ => {}
            }
        }
    }

    if let Some((entity, _, start_node)) = closest {
        dialogue_state.active_npc = Some(entity);
        dialogue_state.current_node = Some(start_node);
        dialogue_state.typewriter_progress = 0.0;
    }
}

/// System to advance typewriter effect.
pub fn typewriter_system(time: Res<Time>, mut dialogue_state: ResMut<DialogueState>) {
    if dialogue_state.active_npc.is_some() && dialogue_state.typewriter_progress < 1.0 {
        dialogue_state.typewriter_progress += time.delta_secs() * 30.0; // ~30 chars/sec
        dialogue_state.typewriter_progress = dialogue_state.typewriter_progress.min(1.0);
    }
}

// Import from sibling modules
use super::npc::Npc;
use super::player::Player;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialogue_tree_from_params() {
        let params = SetDialogueParams {
            npc_id: "merchant".to_string(),
            nodes: vec![
                DialogueNodeDef {
                    id: "start".to_string(),
                    text: "Hello!".to_string(),
                    speaker: None,
                    choices: vec![
                        DialogueChoiceDef {
                            text: "Buy".to_string(),
                            next_node_id: Some("shop".to_string()),
                        },
                        DialogueChoiceDef {
                            text: "Goodbye".to_string(),
                            next_node_id: None,
                        },
                    ],
                },
                DialogueNodeDef {
                    id: "shop".to_string(),
                    text: "What would you like?".to_string(),
                    speaker: Some("Merchant".to_string()),
                    choices: vec![],
                },
            ],
            start_node: "start".to_string(),
            trigger: "proximity".to_string(),
            trigger_radius: 5.0,
        };

        let tree = DialogueTree::from(params);
        assert_eq!(tree.start_node, "start");
        assert_eq!(tree.trigger, DialogueTrigger::Proximity);
        assert_eq!(tree.trigger_radius, 5.0);
        assert_eq!(tree.nodes.len(), 2);

        let start = tree.get_start_node().unwrap();
        assert_eq!(start.text, "Hello!");
        assert_eq!(start.choices.len(), 2);
        assert_eq!(start.choices[0].text, "Buy");
        assert_eq!(start.choices[0].next_node_id, Some("shop".to_string()));

        let shop = tree.get_node("shop").unwrap();
        assert_eq!(shop.speaker, Some("Merchant".to_string()));
    }

    #[test]
    fn test_dialogue_trigger_default() {
        assert_eq!(DialogueTrigger::default(), DialogueTrigger::Click);
    }

    #[test]
    fn test_dialogue_state_default() {
        let state = DialogueState::default();
        assert!(state.active_npc.is_none());
        assert!(state.current_node.is_none());
        assert!(!state.is_typing);
    }

    #[test]
    fn test_dialogue_trigger_parse() {
        // Click is default for unknown strings
        let tree = DialogueTree::from(SetDialogueParams {
            npc_id: "npc".to_string(),
            nodes: vec![],
            start_node: "start".to_string(),
            trigger: "unknown".to_string(),
            trigger_radius: 3.0,
        });
        assert_eq!(tree.trigger, DialogueTrigger::Click);
    }

    #[test]
    fn test_dialogue_tree_get_missing_node() {
        let tree = DialogueTree::default();
        assert!(tree.get_node("nonexistent").is_none());
        assert!(tree.get_start_node().is_none());
    }

    #[test]
    fn test_dialogue_node_def_conversion() {
        let def = DialogueNodeDef {
            id: "test".to_string(),
            text: "Hello world".to_string(),
            speaker: Some("NPC".to_string()),
            choices: vec![
                DialogueChoiceDef {
                    text: "Option A".to_string(),
                    next_node_id: Some("next".to_string()),
                },
                DialogueChoiceDef {
                    text: "Option B".to_string(),
                    next_node_id: None,
                },
            ],
        };
        let node: DialogueNode = def.into();
        assert_eq!(node.id, "test");
        assert_eq!(node.speaker, Some("NPC".to_string()));
        assert_eq!(node.choices.len(), 2);
        assert!(node.choices[1].next_node_id.is_none());
    }

    #[test]
    fn test_set_dialogue_params_default_trigger_radius() {
        assert_eq!(default_trigger_radius(), 3.0);
    }

    #[test]
    fn test_dialogue_state_fields() {
        let state = DialogueState {
            is_typing: true,
            typewriter_progress: 0.5,
            ..Default::default()
        };
        assert!(state.is_typing);
        assert_eq!(state.typewriter_progress, 0.5);
    }
}

// ---------------------------------------------------------------------------
// GAP-P1-02: Dialogue UI Panel
// ---------------------------------------------------------------------------

/// Marker component for the root dialogue UI panel entity.
#[derive(Component)]
pub struct DialoguePanel;

/// Marker for the speaker name text entity.
#[derive(Component)]
pub struct DialogueSpeakerText;

/// Marker for the dialogue body text entity.
#[derive(Component)]
pub struct DialogueBodyText;

/// Marker for choice button entities, storing choice index.
#[derive(Component)]
pub struct DialogueChoiceButton {
    pub index: usize,
}

/// System: spawn, update, and despawn the dialogue UI panel.
///
/// When `DialogueState.active_npc` is Some and no panel exists, spawns the UI.
/// When dialogue ends (active_npc is None), despawns the panel.
/// While active, updates the text content with typewriter effect.
pub fn dialogue_ui_system(
    dialogue_state: Res<DialogueState>,
    npc_query: Query<(&Name, &DialogueTree), With<Npc>>,
    panel_query: Query<Entity, With<DialoguePanel>>,
    mut body_query: Query<&mut Text, (With<DialogueBodyText>, Without<DialogueSpeakerText>)>,
    mut commands: Commands,
) {
    let has_panel = !panel_query.is_empty();

    if let Some(npc_entity) = dialogue_state.active_npc {
        let Some(node_id) = &dialogue_state.current_node else {
            return;
        };

        let Ok((name, tree)) = npc_query.get(npc_entity) else {
            return;
        };

        let Some(node) = tree.get_node(node_id) else {
            return;
        };

        if !has_panel {
            // Spawn dialogue panel
            let speaker = node.speaker.as_deref().unwrap_or(name.as_str());

            commands
                .spawn((
                    DialoguePanel,
                    Node {
                        width: Val::Percent(80.0),
                        height: Val::Auto,
                        min_height: Val::Px(120.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(20.0),
                        left: Val::Percent(10.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
                    ZIndex(50),
                ))
                .with_children(|parent| {
                    // Speaker name
                    parent.spawn((
                        DialogueSpeakerText,
                        Text::new(speaker.to_string()),
                        TextFont {
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.85, 0.3)),
                        Node {
                            margin: UiRect::bottom(Val::Px(8.0)),
                            ..default()
                        },
                    ));
                    // Dialogue body text
                    parent.spawn((
                        DialogueBodyText,
                        Text::new(String::new()),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        Node {
                            margin: UiRect::bottom(Val::Px(12.0)),
                            ..default()
                        },
                    ));
                    // Choice buttons
                    for (i, choice) in node.choices.iter().enumerate() {
                        parent.spawn((
                            DialogueChoiceButton { index: i },
                            Text::new(format!("[{}] {}", i + 1, choice.text)),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.7, 0.9, 1.0)),
                            Node {
                                margin: UiRect::bottom(Val::Px(4.0)),
                                ..default()
                            },
                        ));
                    }
                    // If no choices, show "[E] Continue"
                    if node.choices.is_empty() {
                        parent.spawn((
                            DialogueChoiceButton { index: 0 },
                            Text::new("[E] Continue".to_string()),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.7, 0.9, 1.0)),
                        ));
                    }
                });
        } else {
            // Update text with typewriter effect
            let full_text = &node.text;
            let visible_chars =
                (dialogue_state.typewriter_progress * full_text.len() as f32) as usize;
            let display_text: String = full_text.chars().take(visible_chars).collect();

            for mut text in body_query.iter_mut() {
                **text = display_text.clone();
            }
        }
    } else if has_panel {
        // Dialogue ended — despawn panel
        for entity in panel_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// System: handle player choice selection via number keys or E to continue.
pub fn dialogue_choice_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut dialogue_state: ResMut<DialogueState>,
    npc_query: Query<&DialogueTree, With<Npc>>,
    panel_query: Query<Entity, With<DialoguePanel>>,
    mut commands: Commands,
) {
    let Some(npc_entity) = dialogue_state.active_npc else {
        return;
    };
    let Some(node_id) = dialogue_state.current_node.clone() else {
        return;
    };

    // Wait for typewriter to finish
    if dialogue_state.typewriter_progress < 1.0 {
        // Allow skipping typewriter with E
        if keyboard.just_pressed(KeyCode::KeyE) {
            dialogue_state.typewriter_progress = 1.0;
        }
        return;
    }

    let Ok(tree) = npc_query.get(npc_entity) else {
        return;
    };
    let Some(node) = tree.get_node(&node_id) else {
        return;
    };

    // Determine selected choice
    let selected = if node.choices.is_empty() {
        // No choices — E or any number to continue/end
        if keyboard.just_pressed(KeyCode::KeyE) || keyboard.just_pressed(KeyCode::Digit1) {
            Some(None) // End dialogue
        } else {
            None
        }
    } else {
        // Map number keys to choices
        let choice_keys = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
        ];
        choice_keys
            .iter()
            .enumerate()
            .find(|(i, key)| *i < node.choices.len() && keyboard.just_pressed(**key))
            .map(|(i, _)| node.choices[i].next_node_id.clone())
    };

    if let Some(next_node_id) = selected {
        // Despawn current panel so it rebuilds with new content
        for entity in panel_query.iter() {
            commands.entity(entity).despawn();
        }

        if let Some(next_id) = next_node_id {
            // Advance to next node
            dialogue_state.current_node = Some(next_id);
            dialogue_state.typewriter_progress = 0.0;
        } else {
            // End dialogue
            dialogue_state.active_npc = None;
            dialogue_state.current_node = None;
            dialogue_state.proximity_cooldown = 3.0;
        }
    }
}

/// System: suppress player movement while dialogue is active.
///
/// Resets player input to zero when dialogue is happening.
pub fn dialogue_movement_lock_system(
    dialogue_state: Res<DialogueState>,
    mut input_query: Query<&mut super::player::PlayerInput, With<Player>>,
) {
    if dialogue_state.active_npc.is_some() {
        for mut input in input_query.iter_mut() {
            input.move_forward = 0.0;
            input.move_right = 0.0;
            input.jump = false;
            input.run = false;
        }
    }
}

/// Plugin for dialogue systems.
pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DialogueState::default()).add_systems(
            Update,
            (
                proximity_dialogue_trigger_system,
                click_dialogue_system,
                typewriter_system,
                dialogue_ui_system,
                dialogue_choice_system,
                dialogue_movement_lock_system.after(super::player::player_input_system),
            ),
        );
    }
}
