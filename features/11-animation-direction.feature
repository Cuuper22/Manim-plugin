@F11 @direction
Feature: Direct pacing, focus, and camera continuity

  Scenario: Focus the primary reveal
    Given the characteristic roots are the primary reveal
    When that beat plays
    Then competing objects are visually de-emphasized
    And the roots remain fully inside the safe frame
    And their reading hold meets the beat timing

  Scenario: Preserve conceptual continuity
    Given a state vector persists into the companion-matrix beat
    When the transition plays
    Then its motion or transformation preserves identity
    And it does not disappear without a narrative cause

  Scenario: Reconcile timing with narration
    Given narration for a beat is 1.4 seconds longer than its visual motion
    When timing is directed
    Then the beat gains a 1.4 second readable hold
    And later cue boundaries shift consistently
