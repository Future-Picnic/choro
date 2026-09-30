/*
Copyright 2017 Ziadin Givan

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

   http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

https://github.com/givanz/VvvebJs
*/


// Choro adapter: registry/matcher retained; upstream page manager, galleries,
// Bootstrap templates and GUI excluded. See NOTICE.md.
var Vvveb = {Builder: {}, defaultComponent: "_base"};
Vvveb.Components = {
	
	_components: {},
	
	_nodesLookup: {},
	
	_attributesLookup: {},

	_classesLookup: {},
	
	_classesRegexLookup: {},
	
	componentPropertiesElement: "#right-panel .component-properties",

	componentPropertiesDefaultSection: "content",

	get: function(type) {
		return this._components[type];
	},

	updateProperty: function(type, key, value) {
		let properties = this._components[type]["properties"];
		for (property in properties) {
			if (key == properties[property]["key"])  {
				return this._components[type]["properties"][property] = 
				Object.assign(properties[property], value);
			}
		}
	},

	getProperty: function(type, key) {
		let properties = this._components[type] ? this._components[type]["properties"] : [];
		for (property in properties) {
			if (key == properties[property]["key"])  {
				return properties[property];
			}
		}
	},

	add: function(type, data) {
		data.type = type;
		
		this._components[type] = data;
		
		if (data.nodes) {
			for (let i in data.nodes) {	
				this._nodesLookup[ data.nodes[i] ] = data;
			}
		}
		
		if (data.attributes) {
			if (data.attributes.constructor === Array) {
				for (let i in data.attributes) {	
					this._attributesLookup[ data.attributes[i] ] = data;
				}
			} else {
				for (let i in data.attributes) {	
					if (typeof this._attributesLookup[i] === 'undefined') {
						this._attributesLookup[i] = {};
					}

					if (typeof this._attributesLookup[i][ data.attributes[i] ] === 'undefined') {
						this._attributesLookup[i][ data.attributes[i] ] = {};
					}

					this._attributesLookup[i][ data.attributes[i] ] = data;
				}
			}
		}
		
		if (data.classes) {
			for (let i in data.classes) {	
				this._classesLookup[ data.classes[i] ] = data;
			}
		}
		
		if (data.classesRegex) {
			for (let i in data.classesRegex) {	
				this._classesRegexLookup[ data.classesRegex[i] ] = data;
			}
		}
	},
	
	extend: function(inheritType, type, data) {
		 
		 let newData = data;
		 
		 if (inheritData = this._components[inheritType]) {
			newData = {...inheritData, ...data};
			newData.properties = (data.properties ? data.properties : []).concat(inheritData.properties ? inheritData.properties : []);
		 }

		 //sort by order
		 newData.properties.sort(function (a,b) {
				if (typeof a.sort  === "undefined") a.sort = 0;
				if (typeof b.sort  === "undefined") b.sort = 0;

				if (a.sort < b.sort)
					return -1;
				if (a.sort > b.sort)
					return 1;
				return 0;
			});
		
		this.add(type, newData);
	},
	
	
	matchNode: function(node) {
		let component = {};
		
		if (!node || !node.tagName) return false;
		
		if (node.attributes && node.attributes.length) {
			//search for attributes
			for (let i in node.attributes) {
				if (node.attributes[i]) {
					let attr = node.attributes[i].name;
					let value = node.attributes[i].value;

					if (attr in this._attributesLookup) {
						component = this._attributesLookup[ attr ];
						
						//currently we check that is not a component by looking at name attribute
						//if we have a collection of objects it means that attribute value must be checked
						if (typeof component["name"] === "undefined") {
							if (value in component) {
								return component[value];
							}
						} else {
							return component;
						}
					}
				}
			}
				
			for (let i in node.attributes) {
				let attr = node.attributes[i].name;
				let value = node.attributes[i].value;
				
				//check for node classes
				if (attr == "class") {
					let classes = value.split(" ");
					
					for (let j in classes) {
						if (classes[j] in this._classesLookup)
						return this._classesLookup[ classes[j] ];	
					}
					
					for (let regex in this._classesRegexLookup) {
						let regexObj = new RegExp(regex);
						if (regexObj.exec(value)) {
							return this._classesRegexLookup[ regex ];	
						}
					}
				}
			}
		}

		let tagName = node.tagName.toLowerCase();
		if (tagName in this._nodesLookup) return this._nodesLookup[ tagName ];
	
		return false;
		//return false;
	}
};

Vvveb.WysiwygEditor = {
  isActive: false,
  edit(element) {
    this.element = element;
    this.oldValue = element.innerHTML;
    this.isActive = true;
    element.setAttribute("contenteditable", "true");
    element.setAttribute("spellcheck", "true");
    element.focus();
  },
  destroy(element) {
    element.removeAttribute("contenteditable");
    element.removeAttribute("spellcheck");
    this.isActive = false;
    return {oldValue: this.oldValue, newValue: element.innerHTML};
  }
};
