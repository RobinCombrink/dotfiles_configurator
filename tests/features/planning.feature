Feature: Planning what a machine needs

  A change set says what would change, what is already true, and what cannot be read yet.
  Producing one never changes the machine it describes.

  Scenario: A declared application that is not installed is reported as a change
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "Neovim"

  Scenario: A declared application that is already installed is reported as converged
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A package whose manager is absent is reported as blocked rather than as drift
    Given Alice declares the winget package "Microsoft.PowerShell"
    And winget is absent from Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports 1 blocked resource
    And the change set does not report the machine as converged

  Scenario: A notice is reported whether or not anything has drifted
    Given Alice declares the notice "Sync the Android Studio settings repository"
    When Alice plans
    Then the change set reports 0 changes
    And the change set mentions "Sync the Android Studio settings repository"

  Scenario: An application is reported before the symlink that writes into its directory
    Given Alice declares the symlink "vscode/settings.json" at "settings.json"
    And Alice declares the application "Visual Studio Code"
    And the dotfiles repository has been cloned on Alice's machine
    And Visual Studio Code is not installed on Alice's machine
    When Alice plans
    Then the change set lists the application before the symlink

  Scenario: Planning twice against one machine produces the same change set
    Given Alice declares the application "Neovim"
    And Alice declares the application "Discord"
    And Neovim is not installed on Alice's machine
    And Discord is not installed on Alice's machine
    When Alice plans twice
    Then both change sets are the same

  Scenario: Planning leaves the machine exactly as it was
    Given Alice declares the application "Neovim"
    And Alice declares the winget package "Microsoft.PowerShell"
    And Neovim is not installed on Alice's machine
    When Alice plans
    Then nothing on Alice's machine has changed

  Scenario: A command without a presence check is reported as changing on every run
    Given Alice declares the command "refresh-completions" with no presence check
    When Alice plans
    Then the change set reports 1 change
