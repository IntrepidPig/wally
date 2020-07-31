# Wayland-Alternate

A description of a alternative compositing protocol to Wayland, because that thing is a mess to be perfectly honest.

## Why

The Wayland protocol as it stands exhibits many issues that make it painful to implement compositors and clients that use it. In it's current stage, it's most viable usage is a prototype and proof of concept for a modern desktop compositing protocol. The current moment in time (early 2020) is the ideal time to take all of the lessons learned from making implementations of the Wayland protocol and redesign the protocol from scratch while they are fresh in mind. Now is a better time than any other because, while some significant effort has gone into making fully-featured implementations of the Wayland protocol, not so much work has gone into it that it would be exceedingly painful to have it discarded. As of right now, there are only a few major implementations of the Wayland server protocol, and their use in the real world is far from widespread, yet still great enough to have a wealth of feedback in need of addressing. A protocol redesign will allow such feedback to be addressed, and can be done in a way that minimizes the effort of developers to follow it.

## Current Issues With Wayland

- **Fragmentation**: Being just a protocol, Wayland forces any implementation to implement a lot of functionality that should arguably have a single blessed implementation. One of the greatest advantages of Xorg is the choice it brings to users when it comes to window managers and desktop environments. By implementing only the display server functionality and providing more accessible APIs for developers to create window managers and desktop environments, Xorg has enabled a wealth of choice in the Linux ecosystem. Wayland threatens to harm that level of choice by requiring any window manager or desktop environment implementations to also implement the underlying display and input stack themselves, making it much less feasible for developers without expansive time resources.
- **Incompleteness**: 
- **"Security"**: One of the major selling points of Wayland is it's improved security over Xorg. The only concrete example I have seen of this as of right now is that applications can not keylog other applications on Wayland as they would be able to in Xorg. This argument has always seemed a little incorrect to me. My main concern is that if a desktop already has code running on it untrusted enough to possibly be a keylogger, then the user has much greater concerns than keylogging. I have no experience with this sort of action, but I imagine something of equal or greater harm than keylogging could be done with this sort of privilige, like for example puttings something malicious in `.bashrc` or mocking a legitimate password prompt. My main point is, if a user is running a program that would be keylogging if it weren't for Wayland's protections, then the user has already lost the battle anyway.
- **Protocol clarity**: The Wayland protocol, as it is written right now, leaves a lot to be desired. Having the Vulkan specification and the Wayland protocol specification open side by side makes the Wayland protocol specification look immature and incomplete.
- **Resource management clarity**: The lifetimes of resources as described by the Wayland protocol and wire format are difficult to reason about.
- **Cursor inconsistency**: The Wayland protocol requires clients to set their own pointer image with no knowledge of the compositor's current cursor theme. This is a big issue that apparently hasn't been resolved in a weston-gtk combo yet and can be observed by noticing the inconsistencies in cursors when hovering over a corner vs. when dragging a corner.

## Things That Wayland Has Improved

- **Screen tearing**: Wayland surfaces are double buffered and atomically swapped by design, so as long as the compositor synchronizes it's own presentation with the monitors refresh rate properly, 
- **Asynchronous protocol**: Although this feature adds some complexity overhead to the protocol itself by requiring monotonically increasing serials to be included in certain messages, it is likely a net benefit regarding latency and possibly other things as well.
- **Minimal**: In the spirit 

## Goals For The Alternative Protocol

- **Rich protocol**: No this is a terrible idea for interop with many languages.