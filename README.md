# Wally

A Wayland compositor with an _epic_ tech stack.

## Description

Have you ever felt that with despite all of the hype around fancy new technologies, it's rare to see anyone actually implement something useful with them? Well, this project is here to scratch that itch for you. Built on a powerful trio of newly born technologies that have yet to gain the same footing as their predecessors, Wally is here to entirely displace all that legacy crap, and replace it with shiny new future things.

Specifically, Wally is built on the following foundation:

- **Wayland**
- **Vulkan**
- **Rust**

I know right? With all three pillars, this bad boy is just about unstoppable.

## But why?

Well... because I can? For a proof of concept? Because it's fun? All of these reasons apply to some degree. Don't let it bother you that this projects reason for existence may be a little unclear. Just accept that fact that it exists, and there's nothing you can do about it.

## Some thoughts

Given that some people's perspectives on these technologies might be somewhat unclear because of their unfamiliarity, I thought I'd offer my own review of each after having done a decent amount of work with them.

Lets start with the good:

### **Vulkan**

Working with Vulkan gives me a warm fuzzy feeling inside. I feel safe. Even though when I forget to end my command buffer recording, it borks my entire system to the point of no return, I at least know that the validation layers will give me a nice explanation about what happened.

Another great thing about Vulkan is it's extensibility and flexibility. The API enforces the absolute minimum restrictions on you as a programmer, allowing you to architecht your framework in whatever way most suits the problem at hand. Resource management is clear and easy to follow, allowing you to know exactly where and when everything is.

Of course, Vulkan is also a beast of an API to deal with. The lines of code necessary for basic actions takes a moment to become accustomed to at first. In a way, though, I am grateful. The increased verbosity pays of in programmer flexibility, and aids a fuller comprehension of what is really happening. If verbosity is the price I have to pay to have a clear, flexible, and powerful API, then I will pay it gladly.

**Overall rating**: 7-(8.333)-10/10

### **Wayland**

Oh man. I have so much to complain about. But first, let me start with my disclaimer:

I would say I mean no disrespect towards the people who have put in their valuable time and effort into supporting this project when I say the things I am about to, but I sort of _do_. After working with the Wayland protocol far more than I ever wanted to now, I am legitimately harboring some ill feelings towards the people that developed it. But I shouldn't get into that. My personal feelings are not what matter here, what matters is what's best for the community.

That said, if I could [scrap this attempt at [scrapping the Xorg display server architecture and redesigning it from scratch] and redesign it from scratch] (brackets added for clarity), I would. This is my hubris speaking, and I don't actually believe I personally could do it better, but I do believe that, in it's current state, the Wayland protocol and specification is inadequate for achieving it's intended purpose.

I won't go into the details here; they will be delegated to the `alternate.md` file located elsewhere in this repository. For now, all I will say is:

**Overall rating**: 3.5-(4.5)-7.5/10

### **Rust**

Alright, let's wash out some of the bad taste Wayland left in our mouths with something good. Rust is here to save the day.

My goodness this is an excellent programming language. I would say it's a work of art, but there are other languages better suited to that title. Rust is more of an industrial product. By that I mean, it is designed to resilient, suited to real-world use cases, and powerful.

**Overall rating**:

8-(9)-9.5/10

The format of the ratings is &lt;minimum&gt;-(&lt;estimated&gt;)-&lt;maximum&gt;/&lt;possible&gt;

Notes: rating system seems a little obsequious (idk what that word means, what I mean is that it seems misogynistic towards software projects, i.e. it's a lazy way of evaluating them despite the included analysis and should be removed completely)