Feature: Loading a configuration

  Several configurations are read together and merged into the one desired state a machine is
  converged against. Each configuration declares which machines it is for, and an invocation names
  which machine it is running on, so where configurations are read from cannot change which of them
  apply. A configuration the tool cannot honour is refused before anything is read from the machine.

  Scenario: A configuration for another machine is left out
    Given Alice has a configuration for personal machines linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for work machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then the desired state holds 1 symlink

  Scenario: A configuration for every machine applies alongside the machine's own
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then the desired state holds 2 symlinks

  Scenario: A machine belonging to no class applies only what is for every machine
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a machine of no class
    Then the desired state holds 1 symlink

  Scenario: A configuration for another machine is refused when it cannot be read
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for work machines declaring format version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"

  Scenario: A configuration written for the superseded format is refused by version
    Given Alice has a configuration declaring format version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions "3"

  Scenario: Two configurations claiming the same link differently are refused
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration linking ".gitconfig" to "elsewhere/.gitconfig"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "conflicting claims"

  Scenario: A run reports every configuration it could not read
    Given Alice has a configuration declaring format version "0.1.0"
    And Alice has a configuration declaring format version "0.2.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions "0.2.0"

  Scenario: A configuration Alice can read is refused alongside one she cannot
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration declaring format version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And no desired state is loaded

  Scenario: A refusal names the setting at fault, not only the configuration
    Given Alice has a configuration whose machine settings omit the repositories directory path
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "machine"
    And the refusal mentions "repositories_directory_path"

  Scenario: A configuration declaring no machines it is for is refused
    Given Alice has a configuration that declares no machines it is for
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "applies_to"

  Scenario: Two configurations claiming the same link identically are one resource
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    When Alice loads her configurations for a personal machine
    Then the desired state holds 1 symlink
