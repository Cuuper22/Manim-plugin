@F04 @storyboard
Feature: Build a timed educational storyboard

  Scenario: Structure the generalized Fibonacci narrative
    Given the objective is intuition followed by derivation
    When the director storyboards examples/generalized-fibonacci
    Then the ordered beats are hook, family, data, state-space, roots, edge-case, and recap
    And each beat names one teaching objective and one visual
    And approximation, exact claims, and edge cases remain distinguishable

  Scenario: Honor the duration budget
    Given a brief targets 35 seconds
    When beat durations are assigned
    Then their total including holds and transitions is between 33.25 and 36.75 seconds
