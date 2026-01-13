import AppKit
import SwiftUI
import AppDebugKit

@main
struct TestApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Activate the app and bring to front
        NSApp.activate(ignoringOtherApps: true)

        // Start the debug server
        Task { @MainActor in
            do {
                try AppDebugKit.shared.start()
            } catch {
                print("Failed to start debug server: \(error)")
            }
        }
    }
}

struct ContentView: View {
    @State private var textFieldValue = ""
    @State private var counter = 0
    @State private var isToggleOn = false
    @State private var selectedOption = "Option 1"
    @State private var sliderValue = 50.0
    @State private var statusMessage = "Ready"

    let options = ["Option 1", "Option 2", "Option 3"]

    var body: some View {
        VStack(spacing: 20) {
            Text("AppDebugKit Test App")
                .font(.largeTitle)
                .fontWeight(.bold)
                .debugIdentifier("title")

            Divider()

            // Text field section
            GroupBox("Text Input") {
                HStack {
                    TextField("Enter text here...", text: $textFieldValue)
                        .textFieldStyle(.roundedBorder)
                        .debugIdentifier("textField")

                    Button("Clear") {
                        textFieldValue = ""
                        statusMessage = "Text cleared"
                    }
                    .debugIdentifier("clearButton")
                }
            }

            // Counter section
            GroupBox("Counter") {
                HStack(spacing: 20) {
                    Button("-") {
                        counter -= 1
                        statusMessage = "Counter decremented"
                    }
                    .debugIdentifier("decrementButton")
                    .buttonStyle(.bordered)

                    Text("\(counter)")
                        .font(.title)
                        .frame(minWidth: 60)
                        .debugIdentifier("counterLabel")

                    Button("+") {
                        counter += 1
                        statusMessage = "Counter incremented"
                    }
                    .debugIdentifier("incrementButton")
                    .buttonStyle(.bordered)
                }
            }

            // Toggle section
            GroupBox("Toggle") {
                Toggle("Enable feature", isOn: $isToggleOn)
                    .debugIdentifier("featureToggle")
                    .onChange(of: isToggleOn) { _, newValue in
                        statusMessage = "Feature \(newValue ? "enabled" : "disabled")"
                    }
            }

            // Picker section
            GroupBox("Selection") {
                Picker("Choose option", selection: $selectedOption) {
                    ForEach(options, id: \.self) { option in
                        Text(option).tag(option)
                    }
                }
                .pickerStyle(.segmented)
                .debugIdentifier("optionPicker")
                .onChange(of: selectedOption) { _, newValue in
                    statusMessage = "Selected: \(newValue)"
                }
            }

            // Slider section
            GroupBox("Slider") {
                VStack {
                    Slider(value: $sliderValue, in: 0...100)
                        .debugIdentifier("slider")
                    Text("Value: \(Int(sliderValue))")
                        .debugIdentifier("sliderValue")
                }
            }

            Divider()

            // Action buttons
            HStack(spacing: 20) {
                Button("Submit") {
                    statusMessage = "Submitted: \(textFieldValue)"
                }
                .debugIdentifier("submitButton")
                .buttonStyle(.borderedProminent)

                Button("Reset All") {
                    textFieldValue = ""
                    counter = 0
                    isToggleOn = false
                    selectedOption = "Option 1"
                    sliderValue = 50.0
                    statusMessage = "All values reset"
                }
                .debugIdentifier("resetButton")
                .buttonStyle(.bordered)
            }

            Spacer()

            // Status bar
            HStack {
                Text("Status:")
                    .foregroundColor(.secondary)
                Text(statusMessage)
                    .debugIdentifier("statusLabel")
            }
            .font(.caption)
        }
        .padding(30)
        .frame(minWidth: 500, minHeight: 600)
    }
}

#Preview {
    ContentView()
}
